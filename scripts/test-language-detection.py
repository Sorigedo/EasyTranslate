#!/usr/bin/env python3
"""Regression: short OCR fragments must not select a different language per line."""
from pathlib import Path
import argparse
import json
import subprocess
parser=argparse.ArgumentParser()
parser.add_argument("--bridge",type=Path,default=Path(__file__).resolve().parents[1]/"dist/EasyTranslate.app/Contents/MacOS/TranslationBridge")
args=parser.parse_args()
bridge=args.bridge.resolve()
if not bridge.is_file():
    parser.error(f"Native helper does not exist: {bridge}")
cases=[("Ra\nPlease confirm invoice number 7351.","en"),("• 截\n•\nRa\nPlease confirm invoice number 7351.","en"),("请明天下午联系我，我们需要确认会议安排。","zh-Hans")]
cases.append(("\n\n".join(f"Please confirm invoice number {7351+i}." for i in range(30)),"en"))
cases.append((("请明天确认收到货物。\n"*30)+"Best regards, Jonathan Smith","zh-Hans"))
for sample,expected in cases:
    result=subprocess.run([str(bridge)],input=json.dumps({"action":"detect","text":sample,"source":"auto","target":"zh-Hans"}),text=True,capture_output=True,check=True,timeout=15)
    response=json.loads(result.stdout)
    assert response.get("text")==expected,response
print("PASS: whole-document language detection tolerates incidental OCR fragments")
