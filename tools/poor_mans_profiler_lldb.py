import lldb
import time

# we want async, so we can stop after continuing. This is default though?
#debugger = lldb.SBDebugger.Create()
#debugger.SetAsync(True)
lldb.process.Continue()
times = 0
start = time.time()
for i in range(1,100):
    event = lldb.SBEvent()
    # wait 1s or until event (such as the async interrupt arriving) hits
    if lldb.debugger.GetListener().WaitForEvent(1, event):
        print("GOT EVENT!")
        print(event)

    lldb.process.SendAsyncInterrupt()
    # print the backtrace
    for frame in lldb.thread:
        print(frame)
    while not lldb.thread.Resume():
        pass
    lldb.process.Continue()
    print("BT %d, %f!" % (i, (time.time()-start)*1.0/i))
    time.sleep(1)
