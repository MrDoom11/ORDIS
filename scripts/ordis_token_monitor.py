"""Publish sanitized engine metrics without changing Tabby's streams or usage.

Importing this module uses only the standard library. Call install() from the
native async entrypoint, after Tabby has configured its allocator and config.
The output describes the whole inference engine, including concurrent clients.
"""
from __future__ import annotations

import atexit
from collections import OrderedDict
import functools
import json
import math
import os
from pathlib import Path
import tempfile
import threading
import time


JSON_BYTES = 2048


def _count(value):
    return value if type(value) is int and value >= 0 else None


def _number(value):
    if isinstance(value, bool) or not isinstance(value, (int, float, str)):
        return None
    try:
        value = float(value)
    except (TypeError, ValueError, OverflowError):
        return None
    return value if math.isfinite(value) and value >= 0 else None


class TokenMonitor:
    def __init__(self, outputs, status_display, model_class, *, interval=.5,
                 window_seconds=2.0, min_rate_seconds=.3, finish_ttl_seconds=60,
                 max_tracked_jobs=512, clock=time.monotonic, wall_clock=time.time):
        if interval <= 0 or window_seconds <= 0 or not 0 < min_rate_seconds <= window_seconds:
            raise ValueError('Invalid monitor interval or rate window.')
        if finish_ttl_seconds <= 0 or type(max_tracked_jobs) is not int or max_tracked_jobs < 1:
            raise ValueError('Invalid monitor retention limits.')
        self.outputs = tuple(dict.fromkeys(Path(path) for path in outputs))
        self.status_display = status_display
        self.model_class = model_class
        self.interval = interval
        self.window_seconds = window_seconds
        self.min_rate_seconds = min_rate_seconds
        self.finish_ttl_seconds = finish_ttl_seconds
        self.max_tracked_jobs = max_tracked_jobs
        self.clock, self.wall_clock = clock, wall_clock
        self._lock = threading.RLock()
        self._stop = threading.Event()
        self._thread = None
        self._hook = None
        self._original_finish = None
        self._seen = {}
        self._finished = OrderedDict()
        self._last_generation = None
        self.publish_errors = 0
        self._registered_exit = False

    def start(self, *, background=True):
        if self._hook is not None:
            return self
        original = self.model_class.handle_finish_chunk
        if getattr(original, '_ordis_token_monitor', None) is not None:
            raise RuntimeError('A token monitor is already installed on this model class.')
        monitor = self

        @functools.wraps(original)
        def finish(instance, *args, **kwargs):
            # Return the exact original object. Metrics must never alter
            # inference errors, the finish reason, tools, text, or usage.
            result = original(instance, *args, **kwargs)
            try:
                request_id = kwargs.get('request_id')
                if request_id is None and len(args) > 1:
                    request_id = args[1]
                monitor._record_finish(result, request_id)
            except Exception:
                pass
            return result

        finish._ordis_token_monitor = self
        self._original_finish, self._hook = original, finish
        self.model_class.handle_finish_chunk = finish
        self._stop.clear()
        if background:
            self._thread = threading.Thread(target=self._run, name='ordis-token-monitor', daemon=True)
            self._thread.start()
        atexit.register(self.stop)
        self._registered_exit = True
        return self

    def _record_finish(self, result, request_id):
        if self._stop.is_set() or not isinstance(result, dict):
            return
        tokens = _count(result.get('gen_tokens'))
        if tokens is None:
            return
        request_id = result.get('request_id', request_id)
        record = {
            'tokens': tokens,
            'tokens_per_second': _number(result.get('gen_tokens_per_sec')),
            'seconds': _number(result.get('gen_time')),
            'finished_at': self.wall_clock(),
            'outcome': 'completed',
        }
        with self._lock:
            self._last_generation = record
            if isinstance(request_id, (str, int)):
                self._finished[request_id] = self.clock()
                self._finished.move_to_end(request_id)
                while len(self._finished) > self.max_tracked_jobs:
                    self._finished.popitem(last=False)

    def _rate(self, job, now):
        if getattr(job, 'stage', 'queued') != 'generating':
            return None
        try:
            samples = tuple(job.samples)
        except (AttributeError, RuntimeError, TypeError):
            return None
        valid = []
        for sample in samples:
            if not isinstance(sample, (tuple, list)) or len(sample) != 2:
                continue
            timestamp, count = _number(sample[0]), _count(sample[1])
            if timestamp is None or count is None or not 0 <= now - timestamp <= self.window_seconds:
                continue
            if valid and (timestamp < valid[-1][0] or count < valid[-1][1]):
                return None
            valid.append((timestamp, count))
        if len(valid) < 2:
            return None
        elapsed = valid[-1][0] - valid[0][0]
        if elapsed < self.min_rate_seconds:
            return None
        return (valid[-1][1] - valid[0][1]) / elapsed

    def _off_snapshot(self):
        with self._lock:
            last = dict(self._last_generation) if self._last_generation is not None else None
        return {'schema': 1, 'updated_at': self.wall_clock(), 'status': 'off',
                'active_jobs': 0, 'generated_tokens': 0, 'tokens_per_second': None,
                'last_generation': last}

    def snapshot(self):
        """Read counters already maintained by Tabby; never inspect text or labels."""
        if self._stop.is_set():
            return self._off_snapshot()
        # A dict copy avoids iterating while the inference thread adds/removes
        # jobs. A rare copy failure causes a skipped publish, not a false idle.
        jobs = self.status_display.jobs.copy()
        now = self.clock()
        rows = {}
        for request_id, job in jobs.items():
            stage = getattr(job, 'stage', 'queued')
            stage = stage if stage in ('queued', 'prefill', 'generating') else 'queued'
            rows[request_id] = {
                'tokens': _count(getattr(job, 'gen_tokens', None)) or 0,
                'stage': stage, 'rate': self._rate(job, now),
            }
        with self._lock:
            for key, finished_at in tuple(self._finished.items()):
                if now - finished_at > self.finish_ttl_seconds:
                    self._finished.pop(key, None)
            for key in self._seen.keys() - rows.keys():
                if key not in self._finished:
                    # No official finish was observed: show the last observed
                    # count, without inventing final duration or throughput.
                    self._last_generation = {
                        'tokens': self._seen[key]['tokens'],
                        'tokens_per_second': None, 'seconds': None,
                        'finished_at': self.wall_clock(), 'outcome': 'interrupted',
                    }
                self._finished.pop(key, None)
            self._seen = dict(list(rows.items())[:self.max_tracked_jobs])
            last = dict(self._last_generation) if self._last_generation is not None else None
        stages = {row['stage'] for row in rows.values()}
        stage = next((candidate for candidate in ('generating', 'prefill', 'queued') if candidate in stages), 'idle')
        generating = [row for row in rows.values() if row['stage'] == 'generating']
        rates = [row['rate'] for row in generating]
        rate = sum(rates) if rates and all(value is not None for value in rates) else None
        return {'schema': 1, 'updated_at': self.wall_clock(), 'status': stage,
                'active_jobs': len(rows), 'generated_tokens': sum(row['tokens'] for row in rows.values()),
                'tokens_per_second': round(rate, 3) if rate is not None else None,
                'last_generation': last}

    def _publish(self, snapshot):
        # Static servers can stat the old file and open its replacement. Keep
        # every representation the same byte length so Content-Length remains
        # correct across idle/live/final/off transitions. JSON permits trailing
        # whitespace; padding is bytes, not characters, including with UTF-8.
        try:
            payload = json.dumps(snapshot, ensure_ascii=False, allow_nan=False).encode('utf-8')
            if len(payload) > JSON_BYTES:
                raise ValueError('Token snapshot exceeds its fixed representation size.')
            payload = payload.ljust(JSON_BYTES, b' ')
        except (TypeError, ValueError, OverflowError):
            self.publish_errors += 1
            return
        # The worker owns all I/O. One bad output must not prevent the other
        # interface from receiving a complete JSON file.
        for path in self.outputs:
            temporary = None
            try:
                descriptor, temporary = tempfile.mkstemp(prefix='.ordis-tokens-', suffix='.tmp', dir=path.parent)
                with os.fdopen(descriptor, 'wb') as stream:
                    stream.write(payload)
                Path(temporary).replace(path)
            except (OSError, TypeError, ValueError):
                self.publish_errors += 1
            finally:
                if temporary is not None:
                    try:Path(temporary).unlink(missing_ok=True)
                    except OSError:pass

    def tick(self):
        try:
            snapshot = self.snapshot()
            self._publish(snapshot)
            return snapshot
        except Exception:
            # Monitoring is optional and must never abort the inference loop.
            return None

    def _run(self):
        try:
            while not self._stop.is_set():
                self.tick()
                self._stop.wait(self.interval)
        finally:
            self._publish(self._off_snapshot())

    def stop(self):
        self._stop.set()
        if self._hook is not None:
            if self.model_class.handle_finish_chunk is self._hook:
                self.model_class.handle_finish_chunk = self._original_finish
            self._hook = None
        if self._registered_exit:
            atexit.unregister(self.stop)
            self._registered_exit = False
        if self._thread is not None:
            if self._thread is not threading.current_thread():
                self._thread.join(timeout=1.5)
        else:
            self._publish(self._off_snapshot())


def install(outputs, *, status_display=None, model_class=None, start_thread=True, **options):
    """Install after native config/allocator initialization; return a stop handle.

    Pass injected objects and start_thread=False for standard-library tests.
    Missing/unwritable output directories are ignored by the publisher.
    """
    if status_display is None:
        from common.status_display import status_display
    if model_class is None:
        from backends.exllamav3.model import ExllamaV3Container
        model_class = ExllamaV3Container
    return TokenMonitor(outputs, status_display, model_class, **options).start(background=start_thread)
