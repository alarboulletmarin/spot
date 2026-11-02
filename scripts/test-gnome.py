import gi, time
from gi.repository import Gio, GLib
gi.require_version('Gtk','4.0')
from gi.repository import Gtk, Gdk
Gtk.init()
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None)
def call(method,args=None):
 return bus.call_sync('dev.andrea.Spot.Desktop','/dev/andrea/Spot/Desktop','dev.andrea.Spot.Desktop',method,args,None,Gio.DBusCallFlags.NONE,2000,None).unpack()
print('Enable',call('SetClipboardEnabled',GLib.Variant('(b)',[True])))
received=[]
sub=bus.signal_subscribe('dev.andrea.Spot.Desktop','dev.andrea.Spot.Desktop','ClipboardChanged','/dev/andrea/Spot/Desktop',None,Gio.DBusSignalFlags.NONE,lambda *args: received.append(args[-1].unpack()))
window=Gtk.Window(title='Spot bridge integration fixture')
window.set_child(Gtk.Label(label='Isolated clipboard test'))
window.present()
context=GLib.MainContext.default()
def settle(seconds):
 until=time.monotonic()+seconds
 while time.monotonic()<until:
  while context.pending():context.iteration(False)
  time.sleep(.01)
settle(1)
windows=call('ListWindows')[0]
print('Windows',windows)
assert any('fixture' in title for _,title,_ in windows)
for id,title,_ in windows:
 if 'fixture' in title:print('Focus',call('FocusWindow',GLib.Variant('(t)',[id])))
remote='org.gnome.Mutter.RemoteDesktop'
path=bus.call_sync(remote,'/org/gnome/Mutter/RemoteDesktop',remote,'CreateSession',None,None,Gio.DBusCallFlags.NONE,2000,None).unpack()[0]
def remote_call(method,args=None):
 return bus.call_sync(remote,path,remote+'.Session',method,args,None,Gio.DBusCallFlags.NONE,2000,None)
remote_call('Start')
import os
payload=[b'spot-isolated-clipboard-fixture']
def transfer(*args):
 mime,serial=args[-1].unpack()
 result,fds=bus.call_with_unix_fd_list_sync(remote,path,remote+'.Session','SelectionWrite',GLib.Variant('(u)',[serial]),None,Gio.DBusCallFlags.NONE,2000,None,None)
 fd=fds.get(result.unpack()[0]);os.write(fd,payload[0]);os.close(fd)
 remote_call('SelectionWriteDone',GLib.Variant('(ub)',[serial,True]))
transfer_sub=bus.signal_subscribe(remote,remote+'.Session','SelectionTransfer',path,None,Gio.DBusSignalFlags.NONE,transfer)
remote_call('EnableClipboard',GLib.Variant('(a{sv})',[{'mime-types':GLib.Variant('as',['text/plain'])}]))
settle(1)
print('Clipboard events',received)
assert received and received[-1][0]=='spot-isolated-clipboard-fixture'
# Sensitive offers must be excluded before their text is read.
remote_call('SetSelection',GLib.Variant('(a{sv})',[{'mime-types':GLib.Variant('as',['text/plain','x-kde-passwordManagerHint'])}]))
settle(.3)
assert len(received)==1,received
# Oversized text must not leave the bridge.
payload[0]=b'x'*16385
remote_call('SetSelection',GLib.Variant('(a{sv})',[{'mime-types':GLib.Variant('as',['text/plain'])}]))
settle(.5)
assert len(received)==1,received
print('Disable',call('SetClipboardEnabled',GLib.Variant('(b)',[False])))
remote_call('SetSelection',GLib.Variant('(a{sv})',[{'mime-types':GLib.Variant('as',['text/plain;charset=utf-8'])}]))
settle(.5)
assert len(received)==1,received
window.close()
print('PASS window listing/focus, clipboard opt-in/off')
