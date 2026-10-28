#!/usr/bin/env python3
"""Observe the user's known phone immediately after the bounded radio probe."""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import time


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--send-test', action='store_true')
    args = p.parse_args()
    here = Path(__file__).resolve().parents[2]
    log_path = args.output
    address = 'fe80::d41d:53ff:fed7:71a4'  # Observed user's iPhone, never a subnet scan.
    env = dict(os.environ, AIRBRIDGE_PROBE_SECONDS='120')
    events = []
    with log_path.open('w') as log:
        proc = subprocess.Popen(['/opt/airbridge/bin/airbridge-radio', 'probe',
                                 '/tmp/airbridge-probe', 'AirBridge'], env=env,
                                 stdout=log, stderr=subprocess.STDOUT)
        try:
            start = time.monotonic()
            while time.monotonic() - start < 15:
                if proc.poll() is not None:
                    raise RuntimeError(f'radio startup exited {proc.returncode}')
                if Path('/run/airbridge/radio.ready').exists() and Path('/sys/class/net/awdl0/ifindex').exists():
                    break
                time.sleep(.1)
            scope = socket.if_nametoindex('awdl0')
            last = None
            while time.monotonic() - start < 45:
                if proc.poll() is not None:
                    raise RuntimeError(f'radio exited {proc.returncode}')
                # Prefer a fresh DNS-SD port for this exact phone if one arrived.
                port = 8770
                advertised = False
                for line in log_path.read_text().splitlines():
                    try:
                        item = json.loads(line)
                    except (ValueError, TypeError):
                        continue
                    if item.get('event') == 'receiver-found' and item.get('address') == address:
                        port = item['port']
                        advertised = True
                with socket.socket(socket.AF_INET6, socket.SOCK_STREAM) as sock:
                    sock.settimeout(1)
                    result = sock.connect_ex((address, port, 0, scope))
                state = (port, result, advertised)
                if state != last:
                    item = {'event': 'reverse-connect-probe', 'after_seconds': round(time.monotonic()-start, 2),
                            'port': port, 'errno': result, 'advertised': advertised}
                    print(json.dumps(item), flush=True)
                    events.append(item)
                    last = state
                if result == 0:
                    if args.send_test:
                        send_log = log_path.with_suffix('.send.log')
                        with send_log.open('w') as output:
                            completed = subprocess.run(['/opt/airbridge/bin/airbridge', 'send',
                                '--target', f'[{address}%{scope}]:{port}', '--name', 'AirBridge',
                                str(here/'output/AirBridge-test.txt')], stdout=output,
                                stderr=subprocess.STDOUT, timeout=90)
                        item = {'event': 'send-test-result', 'exit_code': completed.returncode,
                                'log': str(send_log)}
                        events.append(item)
                        print(json.dumps(item), flush=True)
                        print(send_log.read_text(), flush=True)
                    break
                time.sleep(1)
        finally:
            if proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=8)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()
            log_path.with_suffix('.events.json').write_text(json.dumps(events, indent=2)+'\n')


if __name__ == '__main__':
    main()
