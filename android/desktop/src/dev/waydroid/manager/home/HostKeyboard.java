// SPDX-License-Identifier: GPL-3.0-or-later
package dev.waydroid.manager.home;

import android.net.LocalSocket;
import android.net.LocalSocketAddress;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import org.json.JSONObject;

/** Read current Linux state only; no key or text content leaves Android. */
final class HostKeyboard {
    static final class State {
        final boolean russian;
        final boolean caps;
        State(boolean russian, boolean caps) { this.russian = russian; this.caps = caps; }
    }
    static State read() throws Exception {
        try (LocalSocket socket = new LocalSocket()) {
            socket.connect(new LocalSocketAddress(
                    "/data/local/tmp/waydroid-hardware-keyboard/state.sock",
                    LocalSocketAddress.Namespace.FILESYSTEM));
            // Android LocalSocket creates its descriptor in connect(), not in the constructor.
            socket.setSoTimeout(350);
            ByteArrayOutputStream bytes = new ByteArrayOutputStream();
            int value;
            while ((value = socket.getInputStream().read()) != '\n') {
                if (value < 0 || bytes.size() >= 256) throw new IOException("Invalid keyboard state");
                bytes.write(value);
            }
            JSONObject state = new JSONObject(bytes.toString("UTF-8"));
            String layout = state.getString("layout");
            if (!("us".equals(layout) || "ru".equals(layout))
                    || !(state.get("caps") instanceof Boolean)) throw new IOException("Unavailable keyboard state");
            return new State("ru".equals(layout), state.getBoolean("caps"));
        }
    }
}
