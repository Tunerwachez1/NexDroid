// SPDX-License-Identifier: GPL-3.0-or-later
package dev.waydroid.manager.home;

import android.net.LocalSocket;
import android.net.LocalSocketAddress;
import org.json.JSONObject;

/** Metadata only; persistent local connection, bounded ACK, reconnect snapshot. */
final class EditorFocus implements AutoCloseable {
    private LocalSocket socket;
    private String latest = "{\"package\":\"dev.waydroid.manager.home\",\"active\":false}\n";
    private volatile boolean closed;
    EditorFocus() {
        Thread retry = new Thread(() -> {
            while (!closed) {
                synchronized (this) { if (!closed) send(); }
                try { Thread.sleep(500); } catch (InterruptedException ignored) { return; }
            }
        }, "NexDroid-editor-focus");
        retry.setDaemon(true); retry.start();
    }
    synchronized void update(String pkg, boolean active) {
        if (closed) return;
        try {
            JSONObject event = new JSONObject();
            event.put("package", pkg == null ? "dev.waydroid.manager.home" : pkg);
            event.put("active", active);
            latest = event.toString() + "\n";
            send();
        } catch (Exception ignored) { disconnect(); }
    }
    private void send() {
        try {
            if (socket == null) {
                socket = new LocalSocket();
                socket.connect(new LocalSocketAddress("/data/local/tmp/waydroid-hardware-keyboard/focus.sock", LocalSocketAddress.Namespace.FILESYSTEM));
                socket.setSoTimeout(75);
            }
            socket.getOutputStream().write(latest.getBytes("UTF-8"));
            if (socket.getInputStream().read() != '1' || socket.getInputStream().read() != '\n') disconnect();
        } catch (Exception ignored) { disconnect(); }
    }
    private void disconnect() { if (socket != null) try { socket.close(); } catch (Exception ignored) {} socket = null; }
    @Override public synchronized void close() { closed = true; disconnect(); }
}
