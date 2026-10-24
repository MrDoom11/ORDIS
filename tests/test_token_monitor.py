"""Standard-library tests; never import Tabby, Torch, or inference models."""
from collections import deque
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import time
import types
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('ordis_token_monitor', ROOT/'scripts/ordis_token_monitor.py')
BEFORE_IMPORT = set(sys.modules)
monitor_module = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(monitor_module)
IMPORTED_MODULES = set(sys.modules) - BEFORE_IMPORT


class MonitorTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(dir=Path(__file__).parent)
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        for name in ('webui', 'agent'):
            (self.directory/name).mkdir()
        self.outputs = [self.directory/'webui/ordis-tokens.json', self.directory/'agent/ordis-tokens.json']
        self.clock = 10.0
        self.wall = 1_000.0
        self.status = types.SimpleNamespace(jobs={})

        class FakeModel:
            def __init__(self):self.calls=0
            def handle_finish_chunk(self, result, request_id, full_text, label=None):
                self.calls += 1
                if isinstance(result, Exception):raise result
                return result

        self.model_class = FakeModel
        self.model = FakeModel()
        self.original_finish = FakeModel.handle_finish_chunk
        self.monitor = monitor_module.install(
            self.outputs, status_display=self.status, model_class=FakeModel,
            start_thread=False, clock=lambda:self.clock, wall_clock=lambda:self.wall,
        )
        self.addCleanup(self.monitor.stop)

    def job(self, key='secret-request', *, count=20, stage='generating', samples=None):
        job = types.SimpleNamespace(
            gen_tokens=count, stage=stage,
            samples=deque(samples if samples is not None else [(9.0,0),(10.0,count)]),
            label='PRIVATE LABEL AND PROMPT',
        )
        self.status.jobs[key] = job
        return job

    def finish(self, key='secret-request', *, tokens=21, rate=20.1, seconds=1.04):
        result = {'request_id':key, 'gen_tokens':tokens, 'gen_tokens_per_sec':rate,
                  'gen_time':seconds, 'finish_reason':'tool_calls',
                  'full_text':'PRIVATE REASONING AND TOOL ARGUMENTS'}
        self.assertIs(self.model.handle_finish_chunk(result,key,result['full_text']),result)
        return result

    def test_module_import_is_standard_library_only(self):
        self.assertFalse(any(name=='torch' or name.startswith(('torch.','backends.','common.')) for name in IMPORTED_MODULES))

    def test_idle_schema_and_both_outputs_match(self):
        snapshot=self.monitor.tick()
        self.assertEqual(snapshot,{'schema':1,'updated_at':1000.0,'status':'idle',
                                  'active_jobs':0,'generated_tokens':0,'tokens_per_second':None,'last_generation':None})
        self.assertEqual(json.loads(self.outputs[0].read_text()),snapshot)
        self.assertEqual(self.outputs[0].read_bytes(),self.outputs[1].read_bytes())

    def test_fixed_content_length_across_all_states_and_read_replace_race(self):
        states=[]
        def check():
            snapshot=self.monitor.tick()
            for path in self.outputs:
                body=path.read_bytes()
                self.assertEqual(len(body),2048)
                self.assertEqual(json.loads(body),snapshot)
                self.assertTrue(body.startswith(b'{'))
                self.assertEqual(body[len(body.rstrip(b' ')):],b' '*(len(body)-len(body.rstrip(b' '))))
                self.assertNotIn(b'secret-request',body)
                self.assertNotIn(b'PRIVATE',body)
            states.append(snapshot['status'])
        check()
        old_content_length=self.outputs[0].stat().st_size
        job=self.job(stage='queued',count=0);check()
        job.stage='prefill';check()
        job.stage='generating';job.gen_tokens=20;check()
        self.finish(tokens=21);self.status.jobs.clear();check()
        # Simulate a server using an older stat result after opening a newer
        # representation: it still reads the entire valid JSON document.
        self.assertEqual(json.loads(self.outputs[0].read_bytes()[:old_content_length])['last_generation']['tokens'],21)
        self.monitor.stop()
        for path in self.outputs:
            body=path.read_bytes()
            self.assertEqual(len(body),old_content_length)
            self.assertEqual(json.loads(body)['status'],'off')
        self.assertEqual(states,['idle','queued','prefill','generating','idle'])

    def test_oversized_snapshot_leaves_valid_fixed_files_untouched(self):
        self.monitor.tick();before=[path.read_bytes() for path in self.outputs]
        self.monitor._last_generation={'tokens':10**2500,'tokens_per_second':None,'seconds':None,
                                       'finished_at':1000,'outcome':'completed'}
        self.assertIsNotNone(self.monitor.tick())
        self.assertEqual(self.monitor.publish_errors,1)
        self.assertEqual([path.read_bytes() for path in self.outputs],before)

    def test_exact_counter_and_two_second_rate(self):
        self.job(count=20)
        snapshot=self.monitor.snapshot()
        self.assertEqual(snapshot['generated_tokens'],20)
        self.assertEqual(snapshot['tokens_per_second'],20.0)
        self.assertEqual(snapshot['active_jobs'],1)
        self.assertEqual(snapshot['status'],'generating')

    def test_repeated_reads_do_not_add_cumulative_counts(self):
        self.job(count=20)
        for _ in range(4):self.assertEqual(self.monitor.snapshot()['generated_tokens'],20)

    def test_rate_waits_for_two_samples_and_three_tenths(self):
        job=self.job(samples=[(10.0,1)])
        self.assertIsNone(self.monitor.snapshot()['tokens_per_second'])
        job.samples=deque([(9.9,0),(10.0,2)])
        self.assertIsNone(self.monitor.snapshot()['tokens_per_second'])
        job.samples=deque([(9.6,0),(10.0,10)])
        self.assertEqual(self.monitor.snapshot()['tokens_per_second'],25.0)

    def test_stale_samples_do_not_report_a_frozen_rate(self):
        self.job()
        self.assertEqual(self.monitor.snapshot()['tokens_per_second'],20.0)
        self.clock=12.1
        self.assertIsNone(self.monitor.snapshot()['tokens_per_second'])
        self.assertEqual(self.monitor.snapshot()['generated_tokens'],20)

    def test_invalid_and_regressing_samples_do_not_invent_rates(self):
        job=self.job(samples=[(9,2),(10,1)])
        self.assertIsNone(self.monitor.snapshot()['tokens_per_second'])
        job.samples=deque([(float('nan'),2),(10,True),(10,float('inf'))])
        self.assertIsNone(self.monitor.snapshot()['tokens_per_second'])
        job.samples=deque([(9,7),(10,7)])
        self.assertEqual(self.monitor.snapshot()['tokens_per_second'],0.0)

    def test_queued_and_prefill_are_not_decoding(self):
        self.job(stage='queued',count=0)
        self.assertEqual(self.monitor.snapshot()['status'],'queued')
        self.assertIsNone(self.monitor.snapshot()['tokens_per_second'])
        self.status.jobs['secret-request'].stage='prefill'
        self.assertEqual(self.monitor.snapshot()['status'],'prefill')
        self.assertIsNone(self.monitor.snapshot()['tokens_per_second'])

    def test_concurrent_clients_aggregate_counts_and_rates(self):
        self.job('webui-request',count=20)
        self.job('agent-request',count=10)
        snapshot=self.monitor.snapshot()
        self.assertEqual(snapshot['active_jobs'],2)
        self.assertEqual(snapshot['generated_tokens'],30)
        self.assertEqual(snapshot['tokens_per_second'],30.0)

    def test_aggregate_rate_is_not_a_partial_total(self):
        self.job('known',count=20)
        self.job('new',count=1,samples=[(10,1)])
        snapshot=self.monitor.snapshot()
        self.assertEqual(snapshot['generated_tokens'],21)
        self.assertIsNone(snapshot['tokens_per_second'])

    def test_official_finish_preserves_original_return_and_count(self):
        self.job(count=20);self.monitor.snapshot()
        original=self.finish(tokens=21)
        self.assertEqual(self.model.calls,1)
        self.assertEqual(original['finish_reason'],'tool_calls')
        self.status.jobs.clear()
        final=self.monitor.snapshot()['last_generation']
        self.assertEqual(final,{'tokens':21,'tokens_per_second':20.1,'seconds':1.04,
                                'finished_at':1000.0,'outcome':'completed'})

    def test_finish_between_polls_is_still_captured(self):
        self.finish(tokens=3,rate=2,seconds=1.5)
        self.assertEqual(self.monitor.snapshot()['last_generation']['tokens'],3)

    def test_native_indeterminate_rate_stays_null(self):
        self.finish(tokens=1,rate='Indeterminate',seconds=0)
        last=self.monitor.snapshot()['last_generation']
        self.assertIsNone(last['tokens_per_second']);self.assertEqual(last['seconds'],0.0)

    def test_removal_without_finish_is_interrupted_without_invented_mean(self):
        self.job(count=17);self.monitor.snapshot()
        self.status.jobs.clear();self.wall=1001
        final=self.monitor.snapshot()['last_generation']
        self.assertEqual(final,{'tokens':17,'tokens_per_second':None,'seconds':None,
                                'finished_at':1001,'outcome':'interrupted'})

    def test_completed_job_does_not_reset_other_active_client(self):
        self.job('a',count=20);self.job('b',count=10);self.monitor.snapshot()
        self.finish('a',tokens=21);self.status.jobs.pop('a')
        snapshot=self.monitor.snapshot()
        self.assertEqual(snapshot['active_jobs'],1);self.assertEqual(snapshot['generated_tokens'],10)
        self.assertEqual(snapshot['last_generation']['outcome'],'completed')

    def test_snapshot_contains_no_prompts_labels_keys_or_request_ids(self):
        self.job();self.monitor.snapshot();self.finish()
        published=json.dumps(self.monitor.tick())
        for forbidden in ('secret-request','PRIVATE','full_text','request_id','api_key','label'):
            self.assertNotIn(forbidden,published)

    def test_bad_output_does_not_block_other_output(self):
        self.monitor.outputs=(self.directory/'missing/path.json',self.outputs[1])
        self.assertIsNotNone(self.monitor.tick())
        self.assertEqual(self.monitor.publish_errors,1)
        self.assertEqual(json.loads(self.outputs[1].read_text())['status'],'idle')

    def test_atomic_replace_failure_leaves_previous_json_and_cleans_temporary(self):
        self.monitor.tick();before=self.outputs[0].read_bytes()
        self.job()
        with patch.object(Path,'replace',side_effect=PermissionError('busy reader')):
            self.assertIsNotNone(self.monitor.tick())
        self.assertEqual(self.outputs[0].read_bytes(),before)
        self.assertEqual(self.monitor.publish_errors,2)
        self.assertFalse(list(self.directory.rglob('*.tmp')))

    def test_copy_race_skips_publish_without_false_idle(self):
        self.job();self.monitor.tick();before=self.outputs[0].read_bytes()
        class BadCopy:
            def copy(self):raise RuntimeError('concurrent mutation')
        self.status.jobs=BadCopy()
        self.assertIsNone(self.monitor.tick())
        self.assertEqual(self.outputs[0].read_bytes(),before)

    def test_finish_failures_are_not_changed(self):
        failure=RuntimeError('native failure')
        with self.assertRaises(RuntimeError) as caught:self.model.handle_finish_chunk(failure,'id','')
        self.assertIs(caught.exception,failure)
        self.assertIsNone(self.monitor.snapshot()['last_generation'])

    def test_metric_failure_cannot_change_native_finish(self):
        result={'gen_tokens':4}
        with patch.object(self.monitor,'_record_finish',side_effect=RuntimeError('metrics failed')):
            self.assertIs(self.model.handle_finish_chunk(result,'id',''),result)

    def test_retention_is_bounded_and_expires(self):
        self.monitor.max_tracked_jobs=3
        for index in range(10):self.finish(str(index),tokens=index)
        self.assertEqual(len(self.monitor._finished),3)
        self.clock+=61
        self.monitor.snapshot()
        self.assertEqual(len(self.monitor._finished),0)

    def test_stop_restores_hook_and_publishes_off(self):
        self.finish(tokens=9)
        self.monitor.stop()
        self.assertIs(self.model_class.handle_finish_chunk,self.original_finish)
        final=json.loads(self.outputs[0].read_text())
        self.assertEqual(final['status'],'off')
        self.assertEqual(final['active_jobs'],0)
        self.assertEqual(final['last_generation']['tokens'],9)
        self.monitor.stop()  # Idempotent teardown.

    def test_stop_does_not_overwrite_another_later_hook(self):
        def another(*args,**kwargs):return 'another extension'
        self.model_class.handle_finish_chunk=another
        self.monitor.stop()
        self.assertIs(self.model_class.handle_finish_chunk,another)

    def test_background_stop_publishes_off_without_waiting_for_interval(self):
        self.monitor.stop();self.monitor.interval=60
        self.monitor.start(background=True)
        started=time.monotonic();self.monitor.stop()
        self.assertLess(time.monotonic()-started,1)
        self.assertEqual(json.loads(self.outputs[0].read_text())['status'],'off')


if __name__=='__main__':unittest.main()
