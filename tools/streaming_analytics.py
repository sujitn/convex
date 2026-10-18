#!/usr/bin/env python3
"""Run configured native core calculations after confirmed market stream frames.

One ordered worker, existing HTTP ports, and one atomic local recovery receipt.
No pricing formulas, vendor mapping, execution, queue or new server schema.
"""
import argparse
import contextlib
import copy
from datetime import datetime, timedelta, timezone
import fcntl
import hashlib
import http.client
import ipaddress
import json
import os
from pathlib import Path
import re
import signal
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request

import market_data_adapter as adapter
import market_data_stream as stream
import book_updates
import lifecycle_risk
import issuer_scope

MAX_JSON = 2 * 1024 * 1024
EMPTY = dict(calendars=[], helper_calendars=[], curves=[], calibration=None, fixings=[], publications=[])


class Blocked(Exception):
    """Safe, fixed operational reason; never contains a provider payload."""


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), allow_nan=False).encode()


def digest(value):
    return hashlib.sha256(encoded(value)).hexdigest()


def timestamp(value):
    if not isinstance(value, str) or not value.endswith('Z'):
        raise ValueError('UTC timestamp required')
    return datetime.fromisoformat(value[:-1] + '+00:00')


def iso(value):
    return value.isoformat(timespec='microseconds').replace('+00:00', 'Z')


def load(path):
    with Path(path).open('rb') as source:
        raw = source.read(MAX_JSON + 1)
    if len(raw) > MAX_JSON:
        raise ValueError('JSON byte bound')
    value = json.loads(raw, object_pairs_hook=adapter.unique_object, parse_constant=adapter.reject_constant)
    pending, count = [(value, 0)], 0
    while pending:
        item, depth = pending.pop()
        count += 1
        if depth > 64 or count > 65536:
            raise ValueError('JSON structural bound')
        if isinstance(item, dict):
            pending.extend((v, depth + 1) for v in item.values())
        elif isinstance(item, list):
            pending.extend((v, depth + 1) for v in item)
    return value


def name(value):
    if not isinstance(value, str) or not re.fullmatch(r'[A-Za-z0-9._-]{1,128}', value):
        raise ValueError('bounded name')
    return value


def validate(config, universe):
    """Only orchestration shape; economic/native validation stays in the server."""
    if (set(config) != {'schema_version', 'id', 'plan', 'bindings', 'risks', 'pnl', 'hedge'}
            or type(config['schema_version']) is not int or config['schema_version'] != 1):
        raise ValueError('workflow schema')
    name(config['id'])
    name(config['pnl'])
    plan = config['plan']
    if set(plan) != {'definition', 'as_of', 'known_at', 'curve_sets', 'fixings'}:
        raise ValueError('snapshot plan')
    timestamp(plan['as_of'])
    timestamp(plan['known_at'])
    slots = {(s, p['id']) for s, pins in plan['curve_sets'].items() for p in pins}
    if not slots or len(slots) > 256 or len(config['bindings']) != len(slots):
        raise ValueError('curve binding count')
    bound, fresh = set(), 0
    for binding in config['bindings']:
        if set(binding) != {'set', 'curve_id', 'source', 'follow', 'max_age_ms', 'recipe_key'}:
            raise ValueError('curve binding')
        slot = (name(binding['set']), stream.text_id(binding['curve_id']))
        if slot not in slots or slot in bound or (binding['curve_id'], binding['source']) not in universe[0]:
            raise ValueError('unsubscribed/duplicate binding')
        bound.add(slot)
        if (type(binding['follow']) is not bool or type(binding['max_age_ms']) is not int
                or not 1 <= binding['max_age_ms'] <= 86400000):
            raise ValueError('freshness bound')
        fresh += binding['follow']
        if set(binding['recipe_key']) != {'id', 'version'}:
            raise ValueError('native recipe key')
    fixing_keys = {stream.fixing_key(k) for k in plan['fixings']}
    if not fresh or not universe[1] <= fixing_keys or not stream.subscribed_fixings(fixing_keys, universe):
        raise ValueError('fresh current curves and exact fixing subscription required')
    risks = config['risks']
    if not isinstance(risks, dict) or not 1 <= len(risks) <= 30 or len(set(risks.values())) != len(risks):
        raise ValueError('risk bindings')
    for identity, calc in risks.items():
        stream.text_id(identity)
        name(calc)
        if calc == config['pnl'] or calc == 'hedge' or config['pnl'] == 'hedge':
            raise ValueError('P&L/risk name collision')
    hedge = config['hedge']
    if (set(hedge) != {'schema_version', 'inputs', 'operation'} or hedge['schema_version'] != 1
            or hedge['inputs'] != EMPTY or hedge['operation']['kind'] != 'hedge'
            or hedge['operation']['action'] != {'kind': 'solve'}):
        raise ValueError('native hedge template')
    problem = hedge['operation']['problem']
    if set(problem['expected_holdings']) != set(risks) or len(problem['holdings']) != len(risks):
        raise ValueError('complete hedge holdings')
    if {p['risk'] for p in problem['holdings']} != set(risks) or not 1 <= len(problem['candidates']) <= 256:
        raise ValueError('hedge vector bindings')
    for p in [*problem['holdings'], *problem['candidates']]:
        if p['risk'] not in risks or not isinstance(p['fx'], dict):
            raise ValueError('explicit risk/FX binding')


class Client:
    def __init__(self, url, token=None, timeout=10, replay=False):
        self.url = adapter.endpoint(url)
        self.timeout, self.replay = timeout, replay
        self.headers = {'Content-Type': 'application/json'}
        if token is not None:
            if not token or len(token) > 8192 or any(ord(c) < 33 or ord(c) > 126 for c in token):
                raise ValueError('token')
            self.headers['Authorization'] = 'Bearer ' + token
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), adapter.NoRedirect())

    def call(self, method, path, value=None):
        headers = dict(self.headers)
        if path == '/reference/transactions':
            headers['Idempotency-Key'] = value['idempotency_key']
        request = urllib.request.Request(self.url + path, None if value is None else encoded(value),
                                         headers, method=method)
        try:
            with self.opener.open(request, timeout=self.timeout) as response:
                raw = response.read(16 * MAX_JSON + 1)
                if (response.status != 200 or len(raw) > 16 * MAX_JSON
                        or (self.replay and response.headers.get('x-convex-profile') != 'demo')):
                    raise adapter.IngestionError('Unexpected core calculation response')
                return json.loads(raw, object_pairs_hook=adapter.unique_object, parse_constant=adapter.reject_constant)
        except urllib.error.HTTPError as error:
            status = error.code
            error.close()
            raise adapter.IngestionError('Core calculation request rejected', http_status=status) from None
        except (OSError, urllib.error.URLError, http.client.HTTPException):
            raise adapter.IngestionError('Core calculation acknowledgement unavailable') from None
        except (ValueError, UnicodeError, RecursionError):
            raise adapter.IngestionError('Invalid core calculation response') from None

    def assemble(self, plan):
        return self.call('POST', '/analytics/snapshots', plan)['snapshot_id']

    def run(self, snapshot, calculation):
        path = '/analytics/snapshots/' + urllib.parse.quote(snapshot, safe='') + '/run/' + name(calculation)
        receipt = self.call('POST', path, {})
        result = self.call('GET', '/analytics/results/' + urllib.parse.quote(receipt['result_id'], safe=''))
        if result.get('ok') != 'true':
            raise Blocked('native_calculation_failed')
        return receipt, result['result']['data']


@contextlib.contextmanager
def owner(path):
    """One Unix worker per local receipt, including concurrent process starts."""
    with Path(str(path) + '.lock').open('a+b') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise adapter.IngestionError('Workflow receipt already has an active worker') from None
        try:
            yield
        finally:
            fcntl.flock(lock, fcntl.LOCK_UN)


def save(path, value):
    raw = encoded(value)
    if len(raw) > MAX_JSON:
        raise ValueError('workflow receipt byte bound')
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, prefix='.' + path.name, delete=False) as output:
            temporary = output.name
            output.write(raw)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if temporary is not None:
            Path(temporary).unlink(missing_ok=True)


class Workflow:
    def __init__(self, config, client, path, universe, *, clock=None, mode='live', emit=None):
        validate(config, universe)
        self.config, self.client, self.path = copy.deepcopy(config), client, Path(path)
        self.clock = clock or (lambda: datetime.now(timezone.utc))
        self.mode = mode
        self.emit = emit or (lambda event: print(json.dumps(event), flush=True))
        # An opt-in dated replay advances its clock across restarts. Immutable
        # plans and the observed book high-water mark still forbid rollback.
        digest_mode = 'replay:date-rolls' if len(universe) == 4 and mode.startswith('replay:') else mode
        self.config_digest = digest(dict(config=config, mode=digest_mode))
        self.boot_config, self.universe = copy.deepcopy(config), universe
        self.book, self.pending_book, self.reference_record = None, None, None
        self.book_head = None
        self.hedge_template = self.config['hedge']
        if len(universe) >= 3:
            self.config_digest = digest(dict(config=config, mode=digest_mode, book=universe[2]))
        self.plan = copy.deepcopy(config['plan'])
        self.bundle, self.current, self.records = None, False, {}
        self.last_clock = self.clock()
        if self.path.exists():
            prior = load(self.path)
            required = {'schema_version', 'config_digest', 'plan', 'bundle'}
            if prior.get('schema_version') == 2 and len(universe) >= 3:
                required.update(['book', 'book_head'])
            if (set(prior) != required or type(prior['schema_version']) is not int or prior['schema_version'] not in (1, 2)
                    or prior['config_digest'] != self.config_digest):
                raise ValueError('workflow receipt/config mismatch')
            if 'book_head' in prior:
                head = prior['book_head']
                if (not isinstance(head, dict) or set(head) != {'id', 'version', 'digest'}
                        or head['id'] != universe[2] or type(head['version']) is not int
                        or not 1 <= head['version'] <= 2**64 - 1
                        or not isinstance(head['digest'], str) or not re.fullmatch('[0-9a-f]{64}', head['digest'])):
                    raise ValueError('invalid saved book high-water mark')
                self.book_head = head
            if prior.get('book') is not None:
                release = stream.book_record(dict(schema_version=1, kind='book', payload=prior['book']), universe[2])
                self.validate_book_config(release)
                self.config = copy.deepcopy(release['workflow'])
                self.plan = copy.deepcopy(self.config['plan'])
                self.book = copy.deepcopy(release)
                if (release['version'] > self.book_head['version'] or
                        (release['version'] == self.book_head['version'] and digest(release) != self.book_head['digest'])):
                    raise ValueError('saved active book exceeds its observed high-water mark')
            # Only version high-water marks/cutoffs are restored. A prior ready
            # flag or result is never authority to resume without an image.
            restored = prior['plan']
            baseline = copy.deepcopy(restored)
            baseline['as_of'], baseline['known_at'] = self.plan['as_of'], self.plan['known_at']
            for binding in self.config['bindings']:
                if binding['follow']:
                    p = self.pin(baseline, binding)
                    if type(p['version']) is not int or p['version'] < self.pin(self.plan, binding)['version']:
                        raise ValueError('workflow receipt rollback')
                    p['version'] = self.pin(self.plan, binding)['version']
            if baseline != self.plan or timestamp(restored['as_of']) < timestamp(self.plan['as_of']) or timestamp(restored['known_at']) < timestamp(self.plan['known_at']):
                raise ValueError('workflow receipt plan mismatch')
            self.plan = restored
        self.fixing_cutoff = timestamp(self.plan['known_at'])
        self.unavailable('awaiting_image')

    @staticmethod
    def pin(plan, binding):
        return next(p for p in plan['curve_sets'][binding['set']] if p['id'] == binding['curve_id'])

    def unavailable(self, reason):
        self.current = False
        self.emit(dict(state='unavailable', ready=False, reason=reason, mode=self.mode, execution_authority='none'))

    def persist(self, bundle=None):
        state = dict(schema_version=1, config_digest=self.config_digest, plan=self.plan, bundle=bundle)
        if self.book_head is not None:
            state.update(schema_version=2, book=self.book, book_head=self.book_head)
        save(self.path, state)

    def validate_book_config(self, release):
        config = release['workflow']
        validate(config, self.universe)
        if (config['id'] != self.boot_config['id'] or config['plan']['definition']['id'] != self.boot_config['plan']['definition']['id']):
            raise ValueError('book_scope_changed')

    def observe_book(self, frame):
        """Latch a new release before checking its market or native inputs."""
        if len(self.universe) < 3:
            return
        incoming = [r['payload'] for r in frame['records'] if r['kind'] == 'book']
        if incoming:
            release = stream.book_record(dict(schema_version=1, kind='book', payload=incoming[0]), self.universe[2])
            marker = dict(id=release['id'], version=release['version'], digest=digest(release))
            latest = self.book_head
            if latest is not None:
                if release['version'] < latest['version']:
                    if frame['kind'] == 'update':
                        raise stream.Recover('book_revision_rollback')
                    raise Blocked('book_revision_rollback')
                if release['version'] == latest['version'] and marker['digest'] != latest['digest']:
                    raise adapter.IngestionError('Conflicting immutable book release', http_status=409)
            self.pending_book = copy.deepcopy(release)
            if self.book_head != marker:
                self.book_head = marker
                self.persist(self.bundle)  # Even a rejected release cannot roll back after a crash.
        if self.pending_book is None:
            raise Blocked('book_image_required')

    def select_book(self):
        """A release barrier binds complete positions/terms/policy to one commit."""
        if len(self.universe) < 3:
            return
        release = self.pending_book
        self.validate_book_config(release)
        record = book_updates.resolve(self.client, release)
        config = release['workflow']
        if config['plan']['definition']['version'] < self.plan['definition']['version']:
            raise Blocked('book_reference_rollback')
        if timestamp(record['recorded_at']) > self.clock():
            raise Blocked('future_book_reference')
        template = book_updates.bind(config, record['definition']['definition'], release['position_step'])
        plan = copy.deepcopy(config['plan'])
        for cutoff in ['as_of', 'known_at']:
            plan[cutoff] = iso(max(timestamp(plan[cutoff]), timestamp(self.plan[cutoff])))
        plan['known_at'] = iso(max(timestamp(plan['known_at']), timestamp(record['recorded_at'])))
        old_versions = {(b['curve_id'], b['source']): self.pin(self.plan, b)['version']
                        for b in self.config['bindings'] if b['follow']}
        for binding in config['bindings']:
            if binding['follow']:
                pin = self.pin(plan, binding)
                pin['version'] = max(pin['version'], old_versions.get((binding['curve_id'], binding['source']), 0))
        if self.book is None or digest(self.book) != digest(release):
            self.bundle = None
        self.config, self.plan = copy.deepcopy(config), plan
        self.book, self.reference_record, self.hedge_template = copy.deepcopy(release), record, template

    def freshness(self):
        now = self.clock()
        if now < self.last_clock:
            raise Blocked('clock_regressed')
        self.last_clock = now
        deadlines = []
        for binding in self.config['bindings']:
            if not binding['follow']:
                continue  # Historical P&L inputs are fixed, never current marks.
            record = self.records[(binding['curve_id'], binding['source'])]
            if record['recipe']['key'] != binding['recipe_key']:
                raise Blocked('definition_rebind_required')
            if record['quality'] in ('Invalid', 'Stale'):
                raise Blocked('invalid_data_quality')
            source, received = timestamp(record['source_time']), timestamp(record['recorded_at'])
            if source > now or received > now:
                raise Blocked('future_market_data')
            age = min(binding['max_age_ms'], record['max_age_ms'])
            if type(age) is not int or age < 1:
                raise ValueError('invalid_data_age')
            deadlines.extend([source + timedelta(milliseconds=age), received + timedelta(milliseconds=age)])
        if min(deadlines) <= now:
            raise Blocked('stale_market_data')
        return iso(min(deadlines))

    def status(self, event):
        state = event['state']
        if state in ('awaiting_image', 'recovering', 'stopped', 'publishing'):
            self.unavailable(event.get('reason', state))
            if state in ('awaiting_image', 'recovering', 'stopped'):
                self.records.clear()
        elif state in ('duplicate', 'heartbeat') and self.current:
            try:
                self.freshness()
            except Blocked as error:
                self.unavailable(str(error))
            except (ValueError, KeyError, TypeError):
                self.unavailable('stale_market_data')

    def frame(self, frame):
        """Called only after the entire frame acknowledges on existing ports."""
        self.unavailable('calculating')
        try:
            self.observe_book(frame)
            if frame['kind'] == 'image':
                self.records.clear()
            for item in frame['records']:
                if item['kind'] == 'curve':
                    record = item['payload']
                    key = (record['curve_id'], record['source'])
                    old = self.records.get(key)
                    if frame['kind'] == 'update' and old is not None and record['version'] < old['version']:
                        raise stream.Recover('source_version_rollback')
                    if old is None or record['version'] > old['version']:
                        self.records[key] = copy.deepcopy(record)
                elif item['kind'] == 'fixings':
                    for fixing in item['payload']:
                        if max(timestamp(fixing['published_at']), timestamp(fixing['recorded_at'])) > self.clock():
                            raise Blocked('future_fixing')
                        self.fixing_cutoff = max(self.fixing_cutoff, timestamp(fixing['recorded_at']), timestamp(fixing['published_at']))
            self.select_book()
            if frame['kind'] == 'image' and len(self.universe) == 4:
                present = {stream.fixing_key(row['key']) for item in frame['records']
                           if item['kind'] == 'fixings' for row in item['payload']}
                if not {stream.fixing_key(key) for key in self.plan['fixings']} <= present:
                    raise Blocked('incomplete_current_fixing_image')
            plan = copy.deepcopy(self.plan)
            plan['known_at'] = iso(max(timestamp(plan['known_at']), self.fixing_cutoff))
            for binding in self.config['bindings']:
                if binding['follow']:
                    record = self.records[(binding['curve_id'], binding['source'])]
                    pin = self.pin(plan, binding)
                    if record['version'] < pin['version']:
                        raise Blocked('source_version_rollback')
                    pin['version'] = record['version']
                    plan['as_of'] = iso(max(timestamp(plan['as_of']), timestamp(record['source_time'])))
                    plan['known_at'] = iso(max(timestamp(plan['known_at']), timestamp(record['recorded_at'])))
            self.freshness()
            if max(timestamp(plan['as_of']), timestamp(plan['known_at'])) > self.clock():
                raise Blocked('future_snapshot_cutoff')
            self.plan = plan
            self.persist()  # Recovery knows the admitted selection even mid-run.
            snapshot = self.client.assemble(self.plan)
            saved = self.client.call('GET', '/analytics/snapshots/' + urllib.parse.quote(snapshot, safe=''))
            self.check_current_sets(saved)
            issuer = issuer_scope.captured(saved)
            receipts, risks, prices, states = {}, {}, [], {}
            for identity, calc in sorted(self.config['risks'].items()):
                self.freshness()
                receipt, result = self.client.run(snapshot, calc)
                vector = result['vector']
                if (vector['id'] != identity or result['input']['snapshot_id'] != snapshot
                        or not vector['sensitivities']
                        or any(s['snapshot_id'] != snapshot for s in vector['sensitivities'])):
                    raise Blocked('risk_identity_mismatch')
                risks[identity], receipts[calc] = vector, receipt
                definition = saved['content']['definition']['definition']['definition']
                operation = definition['calculations'][calc]['request']['operation']
                expected_lifecycle = operation['kind'] == 'lifecycle_hedge_risk'
                if expected_lifecycle and 'lifecycle_unpriced' in result:
                    data = definition['calculations'][self.config['pnl']]['request']['operation']['input']
                    lifecycle_risk.checked_pending(result, operation, data)
                    raise Blocked('issuer_recovery_pending' if issuer is not None else
                        'bond_recovery_pending' if operation['product']['kind'] == 'bond' else 'cds_recovery_pending')
                if expected_lifecycle != ('lifecycle' in result):
                    raise Blocked('lifecycle_risk_result_missing')
                if expected_lifecycle:
                    states[identity] = result
                prices.append(dict(trade=identity, basis=result['basis'], reference_position=vector['reference_position'],
                                   reference_measure=vector['sensitivities'][0]['base_pv']))
                self.freshness()
            receipts[self.config['pnl']], pnl = self.client.run(snapshot, self.config['pnl'])
            checked = [lifecycle_risk.checked_state(states[k], pnl['report'],
                definition['calculations'][self.config['risks'][k]]['request']['operation']) for k in sorted(states)]
            self.freshness()
            request = self.hedge(snapshot, risks, checked, issuer)
            key = digest(dict(config=self.config_digest, request=request))
            reference_inputs = copy.deepcopy(EMPTY)
            if issuer is not None:
                reference_inputs['issuer_scope'] = copy.deepcopy(issuer['reference'])
            definition = dict(id='stream.hedge.' + key, reference_inputs=reference_inputs,
                calculations=dict(hedge=dict(request=request, curve_sets=[])),
                effective={'from': self.config['plan']['as_of'], 'until': None},
                provenance=dict(source=self.config['id'], evidence_reference='Confirmed native risk; proposal only'))
            command = dict(idempotency_key='stream.hedge.' + key, changes=[],
                master_changes=[dict(expected_version=None, definition=dict(kind='calculation', definition=definition))])
            commit = self.client.call('POST', '/reference/transactions', command)
            hedge_snapshot = self.client.assemble(dict(definition=dict(id=definition['id'], version=1),
                as_of=self.plan['as_of'], known_at=commit['recorded_at'], curve_sets={}, fixings=[]))
            self.freshness()
            receipts['hedge'], hedge = self.client.run(hedge_snapshot, 'hedge')
            if len({r['build_id'] for r in receipts.values()}) != 1:
                raise Blocked('server_build_changed')
            valid_until = self.freshness()  # Reject results which expired during calculation.
            self.bundle = dict(snapshot_id=snapshot, receipts=receipts, prices=prices, hedge_status=hedge['status'])
            if 'continuity' in pnl:
                self.bundle['pnl_continuity'] = copy.deepcopy(pnl['continuity'])
            if self.book is not None:
                self.bundle['book'] = dict(id=self.book['id'], version=self.book['version'],
                    release_digest=self.book_head['digest'],
                    reference_commit=self.book['reference_commit'], definition=self.plan['definition'],
                    position_state=request['operation']['problem']['position_state'])
            self.persist(self.bundle)
            valid_until = self.freshness()
            self.current = True
            self.emit(dict(state='ready', ready=True, mode=self.mode, execution_authority='none',
                valid_until=valid_until, session=frame['session'], sequence=frame['sequence'], **self.bundle))
        except adapter.IngestionError as error:
            self.unavailable('calculation_unconfirmed')
            if error.http_status is None or error.http_status in (408, 429, 500, 502, 503, 504):
                raise stream.Recover('calculation_unconfirmed') from None
            raise
        except Blocked as error:
            self.unavailable(str(error))
        except book_updates.BookInputError as error:
            self.unavailable(str(error))
        except (ValueError, KeyError, TypeError, StopIteration, OverflowError):
            self.unavailable('calculation_or_input_rejected')

    def check_current_sets(self, saved):
        """Every curve used by current risk must have a live freshness binding."""
        content = saved['content']
        definition = content['definition']['definition']['definition']
        if self.book is not None and content['definition'] != self.reference_record:
            raise Blocked('book_snapshot_reference_mismatch')
        fresh = {(b['set'], b['curve_id']) for b in self.config['bindings'] if b['follow']}
        for calc in self.config['risks'].values():
            spec = definition['calculations'][calc]
            if spec['request']['operation']['kind'] not in ('graph_hedge_risk', 'lifecycle_hedge_risk') or len(spec['curve_sets']) != 1:
                raise ValueError('native risk owner required')
            for record in content['curves'][spec['curve_sets'][0]]:
                if (spec['curve_sets'][0], record['curve_id']) not in fresh:
                    raise ValueError('current_risk_uses_historical_curve')
        if definition['calculations'][self.config['pnl']]['request']['operation']['kind'] != 'book_pnl':
            raise ValueError('native P&L owner required')

    def hedge(self, snapshot, risks, states=(), issuer=None):
        request = copy.deepcopy(self.hedge_template)
        problem = request['operation']['problem']
        problem['snapshot_id'] = snapshot
        for item in [*problem['holdings'], *problem['candidates']]:
            vector = copy.deepcopy(risks[item['risk']])
            vector['fx'] = item.pop('fx')
            vector['fx']['snapshot_id'] = snapshot
            item['risk'] = vector
        if states:
            request['operation']['action'] = dict(kind='solve_lifecycle', states=copy.deepcopy(states))
            matured = {s['matured_instrument']['id'] for s in states if s.get('matured_instrument') is not None}
            if matured:
                problem['candidates'] = [c for c in problem['candidates']
                    if c['risk']['instrument']['id'] not in matured]
            defaulted = {s['default_entity'] for s in states if s.get('default_entity') is not None}
            if defaulted:
                instruments = [s['position']['identity']['instrument']['id'] for s in states
                    if s.get('default_entity') is not None and 'position' in s]
                problem['candidates'] = [c for c in problem['candidates']
                    if not any(r['entity_id'] in defaulted for r in c['risk']['credit_scope'])
                    and c['risk'].get('instrument', {}).get('id') not in instruments]
        return issuer_scope.hedge(request, snapshot, issuer, states) if issuer is not None else request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--feed-url', required=True)
    parser.add_argument('--url', required=True)
    parser.add_argument('--stream-id', required=True)
    parser.add_argument('--subscription', required=True, type=Path)
    parser.add_argument('--workflow', required=True, type=Path)
    parser.add_argument('--state', required=True, type=Path)
    parser.add_argument('--feed-token-env')
    parser.add_argument('--server-token-env')
    parser.add_argument('--timeout', type=float, default=10)
    parser.add_argument('--reconnects', type=int, default=10)
    parser.add_argument('--retry-delay', type=float, default=0.25)
    parser.add_argument('--replay-at', help='Fixed historical clock; loopback demo server only')
    args = parser.parse_args()
    def token(env):
        if env is None:
            return None
        value = os.environ.get(env)
        if value is None:
            raise ValueError('token environment missing')
        return value
    def stop(*_):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, stop)
    stage = 'configuration'
    try:
        clock, mode = None, 'live'
        if args.replay_at:
            fixed = timestamp(args.replay_at)
            for url in [args.url, args.feed_url]:
                host = urllib.parse.urlsplit(adapter.endpoint(url, required_path=None)).hostname
                if host != 'localhost' and not ipaddress.ip_address(host).is_loopback:
                    raise ValueError('replay requires loopback')
            clock, mode = lambda: fixed, 'replay:' + iso(fixed)
        universe = stream.subscription(args.subscription)
        server_token, feed_token = token(args.server_token_env), token(args.feed_token_env)
        with owner(args.state):
            worker = Workflow(load(args.workflow), Client(args.url, server_token, args.timeout, bool(args.replay_at)),
                              args.state, universe, clock=clock, mode=mode)
            stage = 'stream'
            stream.consume(args.feed_url, args.url, args.stream_id, universe, timeout=args.timeout,
                reconnects=args.reconnects, retry_delay=args.retry_delay,
                feed_token=feed_token, server_token=server_token, emit=worker.status, on_frame=worker.frame)
    except KeyboardInterrupt:
        return 0
    except (ValueError, KeyError, TypeError, OSError, RecursionError, adapter.IngestionError) as error:
        print(json.dumps(dict(status='failed', ready=False, stage=stage, error_type=type(error).__name__,
            http_status=error.http_status if isinstance(error, adapter.IngestionError) else None,
            error='Streaming analytics stopped; inspect configuration, permissions and service health')), file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
