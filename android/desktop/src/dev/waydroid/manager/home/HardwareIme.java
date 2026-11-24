// SPDX-License-Identifier: GPL-3.0-or-later
package dev.waydroid.manager.home;

import android.inputmethodservice.InputMethodService;
import android.view.KeyEvent;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.EditorInfo;
import android.os.SystemClock;
import android.widget.Toast;
import android.util.Log;
import java.util.HashSet;
import java.util.Set;

/** Physical keyboard only: no input window, suggestions, logging, storage or network. */
public final class HardwareIme extends InputMethodService {
    private final Set<Integer> consumed = new HashSet<>();
    private long lastError;
    private EditorFocus editorFocus;
    private final android.os.Handler focusHandler = new android.os.Handler(android.os.Looper.getMainLooper());
    private int inputGeneration;
    @Override public void onCreate() {
        super.onCreate();
        editorFocus = new EditorFocus();
        try {
            HostKeyboard.read();
            Log.i("WaydroidKeyboard", "Linux keyboard state connected");
        } catch (Exception unavailable) {
            Log.w("WaydroidKeyboard", "Linux keyboard state unavailable: " + unavailable.getClass().getSimpleName());
        }
    }
    @Override public boolean onEvaluateInputViewShown() { return false; }
    @Override public boolean onEvaluateFullscreenMode() { return false; }
    @Override public boolean onShowInputRequested(int flags, boolean configChange) { return false; }
    @Override public void onStartInput(EditorInfo info, boolean restarting) {
        inputGeneration++;
        focusHandler.removeCallbacksAndMessages(null);
        super.onStartInput(info, restarting); consumed.clear();
        editorFocus.update(info == null ? null : info.packageName,
                info != null && info.inputType != 0 && getCurrentInputConnection() != null);
    }
    @Override public void onFinishInput() {
        consumed.clear();
        EditorInfo info = getCurrentInputEditorInfo();
        final String pkg = info == null ? null : info.packageName;
        final int generation = ++inputGeneration;
        // Restarting/switching editors often calls finish immediately before start.
        focusHandler.postDelayed(() -> {
            if (inputGeneration == generation) editorFocus.update(pkg, false);
        }, 20);
        super.onFinishInput();
    }
    @Override public void onDestroy() { inputGeneration++; focusHandler.removeCallbacksAndMessages(null); if (editorFocus != null) editorFocus.close(); super.onDestroy(); }
    @Override public boolean onKeyDown(int code, KeyEvent event) {
        InputConnection connection = getCurrentInputConnection();
        EditorInfo editor = getCurrentInputEditorInfo();
        if (connection == null || editor == null || editor.inputType == 0
                || event.isCtrlPressed() || event.isAltPressed() || event.isMetaPressed()) return false;
        if (PhysicalKeys.text(code, false, false, false) == null) return false;
        HostKeyboard.State state;
        try { state = HostKeyboard.read(); }
        catch (Exception unavailable) {
            // Keep hardware typing available if the Linux bridge is temporarily offline.
            long now = SystemClock.uptimeMillis();
            if (now - lastError > 3000) {
                Toast.makeText(this, "Нет связи с Linux: используется аппаратный ввод", Toast.LENGTH_SHORT).show();
                lastError = now;
            }
            return false;
        }
        String text = PhysicalKeys.text(code, state.russian, event.isShiftPressed(), state.caps);
        if (text == null) return false;
        if (!connection.commitText(text, 1)) return false;
        consumed.add(code); return true;
    }
    @Override public boolean onKeyUp(int code, KeyEvent event) { return consumed.remove(code); }
}
