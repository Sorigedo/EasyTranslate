#!/usr/bin/env python3
"""Exercise only our disposable editor; never type into a real chat application."""
from pathlib import Path
import subprocess
import json
import sys
import os
import time
root=Path(__file__).resolve().parents[1]
app_path=Path(os.environ.get("EASYTRANSLATE_APP",root/"dist/EasyTranslate.app"))
subprocess.run(["xcrun","swiftc",str(root/"native/AXFixture.swift"),"-o",str(root/".build/AXFixture")],check=True)
subprocess.run(["xcrun","swiftc",str(root/"native/WorkflowCheck.swift"),"-o",str(root/".build/WorkflowCheck")],check=True)
subprocess.run(["xcrun","swiftc",str(root/"native/SettingsCheck.swift"),"-o",str(root/".build/SettingsCheck")],check=True)
config=Path.home()/"Library/Application Support/EasyTranslate/config.json"
backup=config.read_bytes() if config.exists() else None
config.parent.mkdir(parents=True,exist_ok=True)
config.write_text(json.dumps({"target":"en","reading_target":"zh-Hans","provider":"apple"}))
app=None
fixture=None
try:
    app=subprocess.Popen([str(app_path/"Contents/MacOS/easytranslate")],env={**os.environ,"EASYTRANSLATE_TRACE_FOCUS":"1"})
    time.sleep(1)
    subprocess.run([str(root/".build/SettingsCheck"),str(app.pid)]+(["custom"] if "--custom" in sys.argv else []),check=True,timeout=15)
    fixture=subprocess.Popen([str(root/".build/AXFixture")]+(["--static"] if "--static" in sys.argv else []))
    time.sleep(1)
    subprocess.run([str(root/".build/WorkflowCheck"),str(fixture.pid),str(app.pid)]+(["custom"] if "--custom" in sys.argv else [])+(["static"] if "--static" in sys.argv else [])+(["screenshot"] if "--screenshot" in sys.argv else []),check=True,timeout=60)
finally:
    for process in (fixture,app):
        if process is not None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()

    if backup is None:
        config.unlink(missing_ok=True)
    else:
        config.write_bytes(backup)
