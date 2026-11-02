#!/usr/bin/env python3
"""Measure actual X11 MapNotify events, not an internal GTK 'show' callback."""
import argparse
import ctypes as C
import ctypes.util
import json
import hashlib
import os
from pathlib import Path
import select
import statistics
import subprocess
import time

import gi
gi.require_version('Gio', '2.0')
from gi.repository import Gio, GLib


class MapEvent(C.Structure):
    _fields_ = [('type', C.c_int), ('serial', C.c_ulong), ('send_event', C.c_int),
                ('display', C.c_void_p), ('event', C.c_ulong), ('window', C.c_ulong),
                ('override_redirect', C.c_int)]


class Event(C.Union):
    _fields_ = [('type', C.c_int), ('mapped', MapEvent), ('pad', C.c_long * 24)]


class X11:
    def __init__(self):
        self.x = C.CDLL(ctypes.util.find_library('X11'))
        signatures = {
            'XOpenDisplay': (C.c_void_p, [C.c_char_p]),
            'XDefaultRootWindow': (C.c_ulong, [C.c_void_p]),
            'XSelectInput': (C.c_int, [C.c_void_p, C.c_ulong, C.c_long]),
            'XSync': (C.c_int, [C.c_void_p, C.c_int]),
            'XPending': (C.c_int, [C.c_void_p]),
            'XNextEvent': (C.c_int, [C.c_void_p, C.POINTER(Event)]),
            'XConnectionNumber': (C.c_int, [C.c_void_p]),
            'XCloseDisplay': (C.c_int, [C.c_void_p]),
        }
        for name, (restype, argtypes) in signatures.items():
            fn = getattr(self.x, name)
            fn.restype, fn.argtypes = restype, argtypes
        self.display = self.x.XOpenDisplay(None)
        if not self.display:
            raise RuntimeError('Cannot open benchmark X server')
        self.x.XSelectInput(self.display, self.x.XDefaultRootWindow(self.display), 1 << 19)
        self.x.XSync(self.display, False)
        self.fd = self.x.XConnectionNumber(self.display)

    def drain(self):
        while self.x.XPending(self.display):
            event = Event()
            self.x.XNextEvent(self.display, C.byref(event))

    def wait(self, kind, timeout=15):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            while self.x.XPending(self.display):
                event = Event()
                self.x.XNextEvent(self.display, C.byref(event))
                if event.type == kind:
                    return time.perf_counter_ns()
            select.select([self.fd], [], [], max(0, deadline - time.monotonic()))
        raise TimeoutError(f'X11 event {kind} not received')

    def close(self):
        self.x.XCloseDisplay(self.display)


def command_output(argv):
    try:
        return subprocess.check_output(argv, stderr=subprocess.DEVNULL, text=True, timeout=5).strip()
    except (OSError, subprocess.SubprocessError):
        return 'unavailable'


def footprint(root):
    """All current descendants, with RSS and PSS from smaps_rollup in KiB."""
    parents = {}
    for entry in Path('/proc').iterdir():
        if entry.name.isdigit():
            try:
                stat = (entry / 'stat').read_text().rsplit(')', 1)[1].split()
                parents[int(entry.name)] = int(stat[1])
            except (OSError, ValueError, IndexError):
                pass
    pids = {root}
    while True:
        expanded = pids | {pid for pid, parent in parents.items() if parent in pids}
        if expanded == pids:
            break
        pids = expanded
    rss = pss = 0
    for pid in sorted(pids):
        # Permission failures must fail the measurement, not silently undercount it.
        fields = {}
        for line in Path(f'/proc/{pid}/smaps_rollup').read_text().splitlines():
            key, _, value = line.partition(':')
            if key in ('Rss', 'Pss'):
                fields[key] = int(value.split()[0])
        rss += fields['Rss']
        pss += fields['Pss']
    return {'rss_kib': rss, 'pss_kib': pss, 'processes': len(pids)}


def measure(args):
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    name = 'dev.andrea.Spot.Benchmark'
    env = dict(os.environ, SPOT_APP_ID=name)
    x = X11()
    resident = None

    def stop():
        nonlocal resident
        if resident:
            subprocess.run([args.binary, '--quit'], env=env, stdout=subprocess.DEVNULL, check=True, timeout=5)
            resident.wait(timeout=5)
            resident = None

    def start_ready():
        nonlocal resident
        ready = []
        loop = GLib.MainLoop()
        def changed(_bus, _sender, _path, _iface, _signal, parameters):
            _, _, owner = parameters.unpack()
            if owner:
                ready.append(time.perf_counter_ns())
                loop.quit()
        subscription = bus.signal_subscribe('org.freedesktop.DBus', 'org.freedesktop.DBus',
            'NameOwnerChanged', '/org/freedesktop/DBus', name, Gio.DBusSignalFlags.NONE, changed)
        bus.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus',
            'GetId', None, None, Gio.DBusCallFlags.NONE, 2000, None)
        timer = GLib.timeout_add_seconds(15, lambda: (loop.quit(), False)[1])
        began = time.perf_counter_ns()
        resident = subprocess.Popen([args.binary, '--daemon'], env=env)
        loop.run()
        bus.signal_unsubscribe(subscription)
        if not ready:
            raise TimeoutError('Resident did not acquire its D-Bus name')
        GLib.source_remove(timer)
        return (ready[0] - began) / 1e6

    def hide():
        x.drain()
        subprocess.run([args.binary, '--hide'], env=env, check=True, stdout=subprocess.DEVNULL, timeout=5)
        x.wait(18)  # UnmapNotify, excluding hide latency from the next activation.

    def activate():
        x.drain()
        began = time.perf_counter_ns()
        process = subprocess.Popen([args.binary], env=env, stdout=subprocess.DEVNULL)
        mapped = x.wait(19)  # MapNotify
        process.wait(timeout=5)
        return (mapped - began) / 1e6

    ready, cold, warm = [], [], []
    try:
        for _ in range(args.runs):
            ready.append(start_ready())
            # The first activation of a fresh resident is the cold-window measurement.
            cold.append(activate())
            hide()
            stop()
        ready.append(start_ready())
        activate()
        hide()
        for _ in range(args.runs):
            warm.append(activate())
            hide()
        time.sleep(args.idle_seconds)
        memory = footprint(resident.pid)
        binaries = {p.name: p.stat().st_size for p in [Path(args.binary), Path(args.binary).with_name('spot-resident')]}
        cpu = next((line.split(':', 1)[1].strip() for line in Path('/proc/cpuinfo').read_text().splitlines() if line.startswith('model name')), 'unknown')
        return {
            'environment': {'distro': Path('/etc/os-release').read_text(), 'cpu': cpu,
                'logical_cpus': os.cpu_count(), 'kernel': os.uname().release,
                'renderer': command_output(['glxinfo', '-B']), 'gsk_renderer': env.get('GSK_RENDERER'),
                'session': 'private Xvfb / X11', 'home': 'empty temporary HOME and XDG directories',
                'revision': command_output(['git', 'rev-parse', 'HEAD']), 'version': command_output([args.binary, '--version']),
                'tracked_changes': bool(command_output(['git', 'diff', '--name-only'])),
                'binary_sha256': {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in [Path(args.binary), Path(args.binary).with_name('spot-resident')]}},
            'method': {'runs': args.runs, 'idle_seconds': args.idle_seconds,
                'ready': 'process spawn to NameOwnerChanged; acquisition precedes completion of GTK startup',
                'cold': 'first activation of each newly started daemon to X11 MapNotify',
                'warm': 'launcher process spawn to X11 MapNotify after previous UnmapNotify',
                'clock': 'perf_counter_ns; observer/event-delivery latency included',
                'memory': 'sum smaps_rollup of resident and all current descendants; windows hidden'},
            'ready_ms': {'median': statistics.median(ready[:args.runs]), 'samples': ready[:args.runs]},
            'cold_ms': {'median': statistics.median(cold), 'samples': cold},
            'warm_ms': {'median': statistics.median(warm), 'samples': warm},
            'idle': memory, 'binary_bytes': binaries, 'total_binary_bytes': sum(binaries.values()),
        }
    finally:
        stop()
        x.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True)
    parser.add_argument('--runs', type=int, default=20)
    parser.add_argument('--idle-seconds', type=float, default=2)
    args = parser.parse_args()
    if args.runs < 1 or args.idle_seconds < 0:
        parser.error('runs must be positive; idle seconds must be nonnegative')
    read, write = os.pipe()
    xvfb = subprocess.Popen(['Xvfb', '-displayfd', str(write), '-screen', '0', '1280x800x24', '-nolisten', 'tcp'], pass_fds=(write,), stderr=subprocess.DEVNULL)
    os.close(write)
    try:
        if not select.select([read], [], [], 10)[0]:
            raise TimeoutError('Xvfb did not start')
        display = os.read(read, 128).decode().strip()
        if not display.isdigit():
            raise RuntimeError('Xvfb did not provide a display number')
        os.environ['DISPLAY'] = f':{display}'
        print(json.dumps(measure(args), indent=2))
    finally:
        os.close(read)
        xvfb.terminate()
        xvfb.wait(timeout=5)


if __name__ == '__main__':
    main()
