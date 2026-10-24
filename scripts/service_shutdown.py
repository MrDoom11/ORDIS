import _thread, os, pathlib, threading, time
def watch():
    marker=pathlib.Path(os.environ['ORDIS_STOP_FILE'])
    def wait():
        while not marker.exists():time.sleep(.25)
        marker.unlink(missing_ok=True)
        _thread.interrupt_main()
    threading.Thread(target=wait,daemon=True,name='ordis-stop').start()
