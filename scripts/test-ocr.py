#!/usr/bin/env python3
"""Generated images only: no screenshots, UI automation, or model downloads."""
from pathlib import Path
import argparse
import json
import subprocess
import tempfile
import time

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--apple-translation', action='store_true', help='Also test installed English-to-Chinese Apple models')
args = parser.parse_args()
bridge = root / 'dist/EasyTranslate.app/Contents/MacOS/TranslationBridge'

def call(data):
    start = time.monotonic()
    result = subprocess.run([str(bridge)], input=json.dumps(data), text=True, capture_output=True, timeout=30, check=True)
    response = json.loads(result.stdout)
    print(data['action'], round(time.monotonic() - start, 3), 'seconds', flush=True)
    return response

with tempfile.TemporaryDirectory(prefix='easytranslate-ocr-test-') as directory:
    folder = Path(directory)
    cases = [
        ('english', ['Hello, see you tomorrow.', 'Please confirm the meeting time.']),
        ('chinese', ['请不要现在付款，等我确认后再付。', '请在今天下班前发送报价，订单 AB-2026。', '总价是 1,500.25 美元。']),
        ('blank', []),
    ]
    for mode, expected in cases:
        image = folder / (mode + '.png')
        subprocess.run(['xcrun', 'swift', str(root/'native/OCRFixture.swift'), str(image), mode], check=True)
        response = call({'action':'ocr', 'path':str(image)})
        if not expected:
            assert response.get('error') and not response.get('text'), response
            continue
        actual = response.get('text', '').splitlines()
        if mode == 'chinese':
            # Vision may omit visual spacing between Chinese and Latin glyphs.
            # Keep exact punctuation, digits, letters, Chinese characters and line order.
            assert [line.replace(' ', '') for line in actual] == [line.replace(' ', '') for line in expected], response
        else:
            assert actual == expected, response
        if args.apple_translation and mode == 'english':
            status = call({'action':'status', 'source':'en', 'target':'zh-Hans'})
            assert status.get('status') == 'installed', 'Optional Apple test requires installed models; no download attempted'
            translated = call({'action':'translate', 'text':response['text'], 'source':'en', 'target':'zh-Hans'})
            assert '明天' in translated.get('text', '') and '会议' in translated['text'], translated
    invalid = folder/'invalid.png'
    invalid.write_bytes(b'not an image')
    for image in [invalid, folder/'missing.png']:
        response = call({'action':'ocr', 'path':str(image)})
        assert response.get('error') and not response.get('text'), response
print('PASS: Chinese OCR content and English exact text, blank/invalid/missing-image rejection; no translation models required')
