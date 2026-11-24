package dev.waydroid.manager.home;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;
public final class AppCatalogTest {
    private static void eq(Object expected, Object actual) {
        if (!expected.equals(actual)) throw new AssertionError("Expected " + expected + ", got " + actual);
    }
    public static void main(String[] args) {
        List<String> first = AppCatalog.order(Arrays.asList("apk.game", AppCatalog.PLAY, "store.game"), Collections.emptyList());
        eq(Arrays.asList(AppCatalog.PLAY, "apk.game", "store.game"), first);
        eq(Arrays.asList(AppCatalog.PLAY, "store.game", "new.apk"),
                AppCatalog.order(Arrays.asList("new.apk", "store.game", "new.apk"), first));
        eq(Collections.singletonList(AppCatalog.PLAY), AppCatalog.order(Collections.emptyList(), first));
        eq(Arrays.asList(AppCatalog.PLAY, "one"), AppCatalog.order(Arrays.asList("one", "one"), Arrays.asList("one", "one")));
        eq(true, AppCatalog.include("self", "headless.apk", true, false));
        eq(false, AppCatalog.include("self", "system.service", false, false));
        eq(false, AppCatalog.include("self", "self", true, true));
        eq(true, AppCatalog.include("self", "system.app", false, true));
        System.out.println("8 catalog checks passed");
    }
}
