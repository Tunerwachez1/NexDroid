// SPDX-License-Identifier: GPL-3.0-or-later
package dev.waydroid.manager.home;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Source-independent desktop: one tile per package, stable order, Play always first. */
public final class AppCatalog {
    public static final String PLAY = "com.android.vending";
    public static boolean include(String own, String pkg, boolean userInstalled, boolean launchable) {
        return !own.equals(pkg) && (PLAY.equals(pkg) || userInstalled || launchable);
    }
    public static List<String> order(List<String> installed, List<String> previous) {
        Map<String, Boolean> remaining = new LinkedHashMap<>();
        for (String pkg : installed) remaining.put(pkg, true);
        List<String> result = new ArrayList<>();
        // Keep a permanent Play tile even if it is temporarily unavailable during an update.
        result.add(PLAY);
        remaining.remove(PLAY);
        for (String pkg : previous) if (remaining.remove(pkg) != null) result.add(pkg);
        result.addAll(remaining.keySet());
        return result;
    }
    private AppCatalog() {}
}
