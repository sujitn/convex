#!/usr/bin/env python3
"""Publish a normalized JSONL capture through the existing curve and fixing ports.

Each line is {"schema_version":1,"kind":"curve"|"fixings","payload":...}.
Payloads are the existing SnapshotCurveRecipe or a PublishedFixing array. This
adapter performs no financial conversion and never invents source metadata.
Recovery replays the same file; immutable server publications are idempotent.
"""
import argparse
import http.client
import ipaddress
import json
import os
from pathlib import Path
import sys
import urllib.error
import urllib.parse
import urllib.request

MAX_FILE_BYTES = 32 * 1024 * 1024
MAX_RECORD_BYTES = 2 * 1024 * 1024
MAX_RECORDS = 10_000
PATHS = {'curve': '/analytics/curves', 'fixings': '/analytics/fixings'}


class IngestionError(Exception):
    """Safe operational diagnostic; neither credentials nor provider payloads."""
    def __init__(self, message, *, line=None, acknowledged=0, http_status=None):
        super().__init__(message)
        self.line = line
        self.acknowledged = acknowledged
        self.http_status = http_status

    def diagnostic(self):
        return dict(status='failed', error=str(self), line=self.line,
                    acknowledged=self.acknowledged, http_status=self.http_status,
                    recovery='Replay the identical file after resolving the failure; unconfirmed delivery may have committed.')


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate JSON key')
        result[key] = value
    return result


def reject_constant(_):
    raise ValueError('nonfinite JSON number')


def encode_record(record, line=None):
    """Validate one transport envelope, preserving the existing native payload."""
    try:
        if (not isinstance(record, dict)
                or set(record) != {'schema_version', 'kind', 'payload'}
                or type(record['schema_version']) is not int or record['schema_version'] != 1
                or record['kind'] not in PATHS):
            raise ValueError('record envelope')
        payload = record['payload']
        if ((record['kind'] == 'curve' and not isinstance(payload, dict))
                or (record['kind'] == 'fixings' and (not isinstance(payload, list) or not payload))):
            raise ValueError('payload shape')
        pending = [(payload, 0)]
        visited = 0
        while pending:
            value, depth = pending.pop()
            visited += 1
            if visited > 65_536 or depth > 64:
                raise ValueError('JSON bounds')
            if isinstance(value, dict):
                pending.extend((v, depth + 1) for v in value.values())
            elif isinstance(value, list):
                pending.extend((v, depth + 1) for v in value)
        body = json.dumps(payload, ensure_ascii=False, allow_nan=False,
                          separators=(',', ':')).encode('utf-8')
        if len(body) > MAX_RECORD_BYTES:
            raise ValueError('payload size')
        return PATHS[record['kind']], body
    except (ValueError, TypeError, UnicodeError, RecursionError, OverflowError):
        raise IngestionError('Invalid normalized market-data record', line=line) from None


def load_messages(path):
    """Freeze and preflight a bounded file before making any network request."""
    try:
        with Path(path).open('rb') as source:
            raw = source.read(MAX_FILE_BYTES + 1)
    except OSError:
        raise IngestionError('Cannot read market-data capture') from None
    if len(raw) > MAX_FILE_BYTES:
        raise IngestionError('Market-data capture exceeds 32 MiB')
    # Bound line allocation too: a file of newlines must not allocate millions
    # of entries before the record-count guard can reject it.
    lines = raw.split(b'\n', MAX_RECORDS)
    if lines[-1] == b'':
        lines.pop()
    if not lines or len(lines) > MAX_RECORDS:
        raise IngestionError('Expected 1..10000 JSONL records')
    messages = []
    for line, content in enumerate(lines, 1):
        if not content or len(content) > MAX_RECORD_BYTES:
            raise IngestionError('Empty or oversized market-data record', line=line)
        try:
            record = json.loads(content, object_pairs_hook=unique_object,
                                parse_constant=reject_constant)
            messages.append(encode_record(record, line))
        except (ValueError, TypeError, UnicodeError, RecursionError, OverflowError):
            raise IngestionError('Invalid normalized market-data record', line=line) from None
    return messages


def endpoint(url, required_path='/api/v1'):
    try:
        parsed = urllib.parse.urlsplit(url)
        if (parsed.scheme not in ('http', 'https') or not parsed.hostname
                or parsed.username is not None or parsed.password is not None
                or parsed.query or parsed.fragment
                or (required_path is not None and parsed.path.rstrip('/') != required_path)
                or any(ord(c) < 33 or ord(c) > 126 for c in url)):
            raise ValueError('endpoint')
        _ = parsed.port
        try:
            local = ipaddress.ip_address(parsed.hostname).is_loopback
        except ValueError:
            local = parsed.hostname.lower() == 'localhost'
        if parsed.scheme == 'http' and not local:
            raise ValueError('HTTPS required')
    except ValueError:
        raise IngestionError('Use HTTPS, or HTTP on loopback, with the required path and without credentials/query/fragment') from None
    return url.rstrip('/') if required_path is not None else url


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        return None


def publish_file(path, url, *, token=None, timeout=10):
    """Publish in order; stop at the first unacknowledged line, without skipping."""
    return publish_messages(load_messages(path), url, token=token, timeout=timeout)


def publish_messages(messages, url, *, token=None, timeout=10):
    """Publish an already validated bounded image/update through existing ports."""
    base = endpoint(url)
    if token is not None and (not token or len(token) > 8192
                              or any(ord(c) < 33 or ord(c) > 126 for c in token)):
        raise IngestionError('Invalid bearer token')
    if not 0.1 <= timeout <= 120:
        raise IngestionError('Timeout must be between 0.1 and 120 seconds')
    # Explicit destination only: do not forward input or credentials via redirects
    # or silently inherited proxy settings.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    headers = {'Content-Type': 'application/json'}
    if token is not None:
        headers['Authorization'] = 'Bearer ' + token
    acknowledged = 0
    for line, (route, body) in enumerate(messages, 1):
        request = urllib.request.Request(base + route, body, headers, method='POST')
        try:
            with opener.open(request, timeout=timeout) as response:
                if response.status != 204:
                    raise IngestionError('Unexpected publication acknowledgement', line=line,
                                         acknowledged=acknowledged, http_status=response.status)
        except urllib.error.HTTPError as error:
            status = error.code
            error.close()
            raise IngestionError('Market-data publication rejected', line=line,
                                 acknowledged=acknowledged, http_status=status) from None
        except (OSError, urllib.error.URLError, http.client.HTTPException):
            raise IngestionError('Market-data delivery unconfirmed', line=line,
                                 acknowledged=acknowledged) from None
        acknowledged += 1
    return dict(status='passed', records=len(messages), acknowledged=acknowledged,
                delivery='at_least_once', recovery='replay_identical_file')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('capture', type=Path, help='Normalized complete-image JSONL capture')
    parser.add_argument('--url', required=True, help='Server API base, e.g. http://127.0.0.1:8080/api/v1')
    parser.add_argument('--token-env', help='Environment variable containing the bearer token; never pass a token on the command line')
    parser.add_argument('--timeout', type=float, default=10, help='Per-record seconds, 0.1..120')
    args = parser.parse_args()
    try:
        token = None
        if args.token_env is not None:
            token = os.environ.get(args.token_env)
            if token is None:
                raise IngestionError('Bearer-token environment variable is unset')
        print(json.dumps(publish_file(args.capture, args.url, token=token, timeout=args.timeout)))
    except IngestionError as error:
        print(json.dumps(error.diagnostic()), file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
