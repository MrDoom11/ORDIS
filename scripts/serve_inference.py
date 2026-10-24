import pathlib
import runpy
import sys

from service_shutdown import watch


def main():
    watch()
    sys.path.insert(0, str(pathlib.Path.cwd()))
    sys.argv = [str(pathlib.Path.cwd() / "main.py")]
    # Let Tabby initialize its config and CUDA allocator before importing
    # the model through the optional monitor. Keep the native entrypoint.
    namespace = runpy.run_path(sys.argv[0], run_name="ordis_inference_runtime")
    entrypoint = namespace["entrypoint"]
    original_async = entrypoint.__globals__["entrypoint_async"]

    async def with_counter():
        monitor = None
        try:
            from ordis_token_monitor import install
            from token_counter_paths import metric_outputs

            monitor = install(outputs=metric_outputs())
        except Exception as exc:
            print(f"Contador de tokens indisponível ({type(exc).__name__}); inferência preservada.", file=sys.stderr, flush=True)
        try:
            return await original_async()
        finally:
            if monitor is not None:
                monitor.stop()

    entrypoint.__globals__["entrypoint_async"] = with_counter
    entrypoint()


if __name__ == "__main__":
    main()
