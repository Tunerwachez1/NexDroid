// SPDX-License-Identifier: GPL-3.0-or-later
package dev.waydroid.manager.home;

/** Android hardware keycodes -> US/RU text. Contains no Android or editor dependencies. */
public final class PhysicalKeys {
    public static String text(int code, boolean russian, boolean shift, boolean caps) {
        if (code >= 29 && code <= 54) {
            String letters = russian ? "фисвуапршолдьтщзйкыегмцчня" : "abcdefghijklmnopqrstuvwxyz";
            return letter(letters.charAt(code - 29), shift != caps);
        }
        if (code >= 7 && code <= 16) {
            String digits = shift ? (russian ? ")!\"№;%:?*(" : ")!@#$%^&*(") : "0123456789";
            return String.valueOf(digits.charAt(code - 7));
        }
        if (code == 62) return " ";
        if (russian) {
            switch (code) {
                case 68: return letter('ё', shift != caps);
                case 71: return letter('х', shift != caps);
                case 72: return letter('ъ', shift != caps);
                case 74: return letter('ж', shift != caps);
                case 75: return letter('э', shift != caps);
                case 55: return letter('б', shift != caps);
                case 56: return letter('ю', shift != caps);
                case 76: return shift ? "," : ".";
                case 73: return shift ? "/" : "\\";
                default: break;
            }
        }
        switch (code) {
            case 68: return shift ? "~" : "`";
            case 69: return shift ? "_" : "-";
            case 70: return shift ? "+" : "=";
            case 71: return shift ? "{" : "[";
            case 72: return shift ? "}" : "]";
            case 73: return shift ? "|" : "\\";
            case 74: return shift ? ":" : ";";
            case 75: return shift ? "\"" : "'";
            case 55: return shift ? "<" : ",";
            case 56: return shift ? ">" : ".";
            case 76: return shift ? "?" : "/";
            default: return null; // Enter/Delete/Tab/navigation/function keys stay real hardware events.
        }
    }
    private static String letter(char value, boolean upper) {
        return String.valueOf(upper ? Character.toUpperCase(value) : value);
    }
    private PhysicalKeys() {}
}
