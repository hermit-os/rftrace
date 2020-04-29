import threading
import time
import signal
import os

debug = True
gdb.execute('set pagination 0')

def log(*msg):
    if debug:
        print(*msg)

def pause_after_sec(secs, end):
    global pausestart
    log("Thread starting. Stopping after %ss" % secs)
    time.sleep(end-time.time())
    # stop running gdb by sending sigint
    pausestart = time.time()
    os.kill(os.getpid(), signal.SIGINT)
    log("Thread finishing")

def get_bt_and_pause(secs):
    global pauses
    gdb.execute('bt')

    log("\nContinuing Execution for a while. Starting watchdog.")
    thread = threading.Thread(target=pause_after_sec, args=(secs,time.time()+secs))
    thread.start()

    log("Resuming Process.")
    gdb.execute("continue")

    log("Joining Thread. Should not be necessary, since gdb is stopped only when thread is finished")
    thread.join()

pauses = 0
pausestart = time.time()
for i in range(1,1000):
    get_bt_and_pause(1)