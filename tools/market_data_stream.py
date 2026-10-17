#!/usr/bin/env python3
"""Pull sequenced NDJSON market images/updates into existing curve/fixing ports.

Every connection, including process restart, requires a complete subscription
image. Gaps discard the offending update and reconnect. Immutable port writes
supply replay safety; no separate cursor database or financial calculation.
"""
import argparse
import hashlib
import http.client
import json
import os
from pathlib import Path
import signal
import sys
import time
import urllib.error
import urllib.request

import market_data_adapter as adapter

MAX_FRAME_BYTES = 16 * 1024 * 1024
MAX_WIRE_FRAMES = 1_000_000


class Recover(Exception):
    pass


def text_id(value):
    if (not isinstance(value, str) or not value or len(value.encode('utf-8')) > 128
            or value.strip() != value or any(ord(c) < 32 or ord(c) == 127 for c in value)):
        raise ValueError('identifier')
    return value


def fixing_key(value):
    if not isinstance(value, dict) or set(value) != {'index_id', 'observation_date', 'source'}:
        raise ValueError('fixing key')
    return tuple(text_id(value[k]) for k in ('index_id', 'observation_date', 'source'))


def subscription(path):
    try:
        with Path(path).open('rb') as source:
            raw = source.read(256 * 1024 + 1)
        if len(raw) > 256 * 1024:
            raise ValueError('subscription size')
        value = json.loads(raw, object_pairs_hook=adapter.unique_object, parse_constant=adapter.reject_constant)
        if not isinstance(value, dict):
            raise ValueError('subscription object required')
        follow = value.pop('follow_fixing_dates', False)
        if type(follow) is not bool or follow and 'book' not in value:
            raise ValueError('dated fixing subscription requires a book')
        if (set(value) not in ({'schema_version', 'curves', 'fixings'}, {'schema_version', 'curves', 'fixings', 'book'}) or type(value['schema_version']) is not int
                or value['schema_version'] != 1 or not isinstance(value['curves'], list)
                or not isinstance(value['fixings'], list) or len(value['curves']) > 256
                or len(value['fixings']) > 1024):
            raise ValueError('subscription')
        curves = set()
        for c in value['curves']:
            if not isinstance(c, dict) or set(c) != {'curve_id', 'source'}:
                raise ValueError('curve key')
            curves.add((text_id(c['curve_id']), text_id(c['source'])))
        fixings = {fixing_key(k) for k in value['fixings']}
        if (len(curves) != len(value['curves']) or len(fixings) != len(value['fixings'])
                or not (curves or fixings)):
            raise ValueError('empty/duplicate subscription')
        if 'book' in value:
            if follow:
                return curves, fixings, text_id(value['book']), {(k[0], k[2]) for k in fixings}
            return curves, fixings, text_id(value['book'])
        return curves, fixings
    except (OSError, ValueError, TypeError, KeyError, UnicodeError, RecursionError):
        raise adapter.IngestionError('Invalid bounded stream subscription') from None


def subscribed_fixings(keys, universe):
    """An opt-in roll may add dates only for explicitly subscribed index/sources."""
    return keys <= universe[1] or (len(universe) == 4
        and all((key[0], key[2]) in universe[3] for key in keys))


def book_record(record, identity):
    """A full book release points at an already committed reference transaction."""
    if (set(record) != {'schema_version', 'kind', 'payload'} or type(record['schema_version']) is not int
            or record['schema_version'] != 1 or record['kind'] != 'book'):
        raise ValueError('book record')
    payload = record['payload']
    adapter.payload_bytes(payload)
    if (set(payload) != {'id', 'version', 'reference_commit', 'workflow', 'position_step'}
            or payload['id'] != identity or type(payload['version']) is not int
            or not 1 <= payload['version'] <= 2**64 - 1
            or not isinstance(payload['workflow'], dict)
            or type(payload['position_step']) is not int or not 0 <= payload['position_step'] < 16):
        raise ValueError('book release')
    text_id(payload['reference_commit'])
    return payload


def decode(raw, stream_id, universe):
    """Validate the complete frame and subscription coverage before any write."""
    try:
        if not raw or len(raw) > MAX_FRAME_BYTES or not raw.endswith(b'\n'):
            raise ValueError('frame size/termination')
        frame = json.loads(raw, object_pairs_hook=adapter.unique_object, parse_constant=adapter.reject_constant)
        if (set(frame) != {'schema_version', 'stream_id', 'session', 'sequence', 'kind', 'records'}
                or type(frame['schema_version']) is not int or frame['schema_version'] != 1
                or frame['stream_id'] != stream_id or frame['kind'] not in ('image', 'update', 'heartbeat')
                or type(frame['sequence']) is not int or not 1 <= frame['sequence'] <= 2**64 - 1
                or not isinstance(frame['records'], list) or len(frame['records']) > adapter.MAX_RECORDS):
            raise ValueError('frame envelope')
        text_id(frame['session'])
        messages, curves, fixings, books = [], set(), set(), []
        for record in frame['records']:
            if isinstance(record, dict) and record.get('kind') == 'book':
                if len(universe) < 3:
                    raise ValueError('unsubscribed book')
                books.append(book_record(record, universe[2]))
                if len(books) > 1:
                    raise ValueError('duplicate book release')
                continue
            messages.append(adapter.encode_record(record))
            if record['kind'] == 'curve':
                p = record['payload']
                curves.add((text_id(p['curve_id']), text_id(p['source'])))
            else:
                for p in record['payload']:
                    fixings.add(fixing_key(p['key']))
        expected_curves, expected_fixings = universe[:2]
        if not curves <= expected_curves or not subscribed_fixings(fixings, universe):
            raise ValueError('unsubscribed input')
        if frame['kind'] == 'heartbeat':
            if frame['records']:
                raise ValueError('heartbeat carries data')
        elif (not frame['records'] or (frame['kind'] == 'image' and
                (curves != expected_curves or not expected_fixings <= fixings or (len(universe) >= 3 and not books)))):
            raise ValueError('incomplete image/update')
        digest = hashlib.sha256(json.dumps(frame, sort_keys=True, separators=(',', ':'),
                                         allow_nan=False).encode()).hexdigest()
        return frame, messages, digest
    except (ValueError, TypeError, KeyError, UnicodeError, RecursionError, OverflowError, adapter.IngestionError):
        raise adapter.IngestionError('Invalid or incomplete subscribed stream frame') from None


def consume(feed_url, server_url, stream_id, universe, *, feed_token=None, server_token=None,
            timeout=10, reconnects=10, retry_delay=0.25, max_frames=None, emit=None, on_frame=None):
    """One ordered pull loop; only a confirmed full image restores continuity."""
    feed_url = adapter.endpoint(feed_url, required_path=None)
    server_url = adapter.endpoint(server_url)
    try:
        text_id(stream_id)
        if not stream_id.isascii():
            raise ValueError('HTTP stream identifier')
        if (not 0.1 <= timeout <= 120 or not 0 <= reconnects <= 1000
                or not 0.01 <= retry_delay <= 60
                or (max_frames is not None and not 1 <= max_frames <= MAX_WIRE_FRAMES)):
            raise ValueError('bounds')
        for token in [feed_token, server_token]:
            if token is not None and (not token or len(token) > 8192
                                     or any(ord(c) < 33 or ord(c) > 126 for c in token)):
                raise ValueError('token')
    except (ValueError, TypeError):
        raise adapter.IngestionError('Invalid stream configuration') from None
    emit = emit or (lambda state: print(json.dumps(state), flush=True))
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), adapter.NoRedirect())
    headers = {'Accept': 'application/x-ndjson', 'X-Convex-Require-Image': 'true',
               'X-Convex-Stream': stream_id}
    if feed_token is not None:
        headers['Authorization'] = 'Bearer ' + feed_token
    accepted, wire = 0, 0
    try:
        for attempt in range(reconnects + 1):
            emit(dict(state='awaiting_image', image_confirmed=False, accepted_frames=accepted))
            session, sequence, last_digest = None, None, None
            try:
                request = urllib.request.Request(feed_url, headers=headers)
                with opener.open(request, timeout=timeout) as response:
                    if response.status != 200 or response.headers.get_content_type() != 'application/x-ndjson':
                        raise adapter.IngestionError('Unexpected market-stream response')
                    while wire < MAX_WIRE_FRAMES:
                        raw = response.readline(MAX_FRAME_BYTES + 1)
                        if not raw:
                            raise Recover('disconnected')
                        if len(raw) <= MAX_FRAME_BYTES and not raw.endswith(b'\n'):
                            raise Recover('truncated_frame')
                        wire += 1
                        frame, messages, digest = decode(raw, stream_id, universe)
                        kind, current = frame['kind'], frame['sequence']
                        if session is None:
                            if kind != 'image':
                                raise Recover('image_required')
                        elif frame['session'] != session:
                            raise Recover('session_changed')
                        elif kind == 'heartbeat':
                            if current != sequence:
                                raise Recover('sequence_gap')
                            emit(dict(state='heartbeat', image_confirmed=True, accepted_frames=accepted))
                            continue
                        elif current == sequence:
                            if digest != last_digest:
                                raise adapter.IngestionError('Conflicting stream sequence')
                            emit(dict(state='duplicate', image_confirmed=True, accepted_frames=accepted))
                            continue
                        elif current < sequence or (kind != 'image' and current != sequence + 1):
                            raise Recover('sequence_gap')
                        emit(dict(state='publishing', image_confirmed=False, accepted_frames=accepted))
                        try:
                            adapter.publish_messages(messages, server_url, token=server_token, timeout=timeout)
                        except adapter.IngestionError as error:
                            if error.http_status is None or error.http_status in (408, 429, 500, 502, 503, 504):
                                raise Recover('publication_unconfirmed') from None
                            raise
                        session, sequence, last_digest = frame['session'], current, digest
                        accepted += 1
                        if on_frame is not None:
                            on_frame(frame)
                        emit(dict(state='streaming', image_confirmed=True, accepted_frames=accepted,
                                  sequence=sequence, kind=kind, acknowledged_records=len(messages)))
                        if max_frames is not None and accepted >= max_frames:
                            return dict(state='stopped', image_confirmed=False, accepted_frames=accepted)
                    raise adapter.IngestionError('Stream frame budget exhausted')
            except urllib.error.HTTPError as error:
                status = error.code
                error.close()
                if status not in (408, 429, 500, 502, 503, 504):
                    raise adapter.IngestionError('Market-stream request rejected', http_status=status) from None
                reason = 'source_unavailable'
            except (Recover, OSError, urllib.error.URLError, http.client.HTTPException) as error:
                reason = str(error) if isinstance(error, Recover) else 'transport_unavailable'
            emit(dict(state='recovering', image_confirmed=False, reason=reason, accepted_frames=accepted))
            if attempt == reconnects:
                raise adapter.IngestionError('Stream reconnect budget exhausted')
            time.sleep(retry_delay)
    finally:
        emit(dict(state='stopped', image_confirmed=False, accepted_frames=accepted))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--feed-url', required=True)
    parser.add_argument('--url', required=True, help='Existing server /api/v1 URL')
    parser.add_argument('--stream-id', required=True)
    parser.add_argument('--subscription', required=True, type=Path)
    parser.add_argument('--feed-token-env')
    parser.add_argument('--server-token-env')
    parser.add_argument('--timeout', type=float, default=10)
    parser.add_argument('--reconnects', type=int, default=10)
    parser.add_argument('--retry-delay', type=float, default=0.25)
    parser.add_argument('--max-frames', type=int, help='Stop after this many confirmed data frames')
    args = parser.parse_args()
    def token(name):
        if name is None:
            return None
        value = os.environ.get(name)
        if value is None:
            raise adapter.IngestionError('Stream token environment variable is unset')
        return value
    def stop(*_):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, stop)
    try:
        consume(args.feed_url, args.url, args.stream_id, subscription(args.subscription),
                feed_token=token(args.feed_token_env), server_token=token(args.server_token_env),
                timeout=args.timeout, reconnects=args.reconnects, retry_delay=args.retry_delay,
                max_frames=args.max_frames)
    except KeyboardInterrupt:
        return 0
    except adapter.IngestionError as error:
        print(json.dumps(error.diagnostic()), file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
