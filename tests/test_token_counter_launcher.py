import contextlib
import importlib.util
import io
from pathlib import Path
import sys
from types import ModuleType, SimpleNamespace
import unittest
from unittest.mock import patch


class LauncherTests(unittest.TestCase):
    def run_launcher(self, *, install_error=False, backend_error=False):
        trace = []
        namespace = {"trace": trace, "backend_error": backend_error}
        exec("""
async def entrypoint_async():
    trace.append('backend')
    if backend_error:
        raise RuntimeError('native failure')
def entrypoint():
    trace.append('config and allocator ready')
    coroutine = entrypoint_async()
    try:
        coroutine.send(None)
    except StopIteration:
        pass
    finally:
        coroutine.close()
""", namespace)
        monitor_module = ModuleType("ordis_token_monitor")

        def install(**kwargs):
            trace.append("monitor imports backend")
            if install_error:
                raise ValueError("optional monitor unavailable")
            return SimpleNamespace(stop=lambda: trace.append("monitor stopped"))

        monitor_module.install = install
        paths_module = ModuleType("token_counter_paths")
        paths_module.metric_outputs = lambda: []
        shutdown_module = ModuleType("service_shutdown")
        shutdown_module.watch = lambda: trace.append("shutdown watch")
        with patch.dict(sys.modules, {"service_shutdown": shutdown_module, "ordis_token_monitor": monitor_module,
                                      "token_counter_paths": paths_module}):
            spec = importlib.util.spec_from_file_location("counter_launcher_test", Path(__file__).resolve().parents[1] / "scripts/serve_inference.py")
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            original_path, original_argv = list(sys.path), sys.argv
            try:
                with patch.object(module.runpy, "run_path", return_value=namespace), contextlib.redirect_stderr(io.StringIO()):
                    if backend_error:
                        with self.assertRaisesRegex(RuntimeError, "native failure"):
                            module.main()
                    else:
                        module.main()
            finally:
                sys.path[:] = original_path
                sys.argv = original_argv
        return trace

    def test_native_config_runs_before_monitor_can_import_torch(self):
        self.assertEqual(self.run_launcher(), ["shutdown watch", "config and allocator ready", "monitor imports backend", "backend", "monitor stopped"])

    def test_optional_monitor_failure_does_not_block_model(self):
        self.assertEqual(self.run_launcher(install_error=True), ["shutdown watch", "config and allocator ready", "monitor imports backend", "backend"])

    def test_native_error_still_cleans_up_monitor(self):
        self.assertEqual(self.run_launcher(backend_error=True), ["shutdown watch", "config and allocator ready", "monitor imports backend", "backend", "monitor stopped"])


if __name__ == "__main__":
    unittest.main()
