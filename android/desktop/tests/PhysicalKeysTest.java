package dev.waydroid.manager.home;
public final class PhysicalKeysTest {
    private static void eq(String expected, String actual) {
        if (expected == null ? actual != null : !expected.equals(actual)) throw new AssertionError(expected + " != " + actual);
    }
    public static void main(String[] args) {
        eq("q", PhysicalKeys.text(45, false, false, false));
        eq("й", PhysicalKeys.text(45, true, false, false));
        eq("Q", PhysicalKeys.text(45, false, false, true));
        eq("Й", PhysicalKeys.text(45, true, false, true));
        eq("й", PhysicalKeys.text(45, true, true, true));
        eq("Ф", PhysicalKeys.text(29, true, true, false));
        eq("ё", PhysicalKeys.text(68, true, false, false));
        eq("Ё", PhysicalKeys.text(68, true, false, true));
        eq("№", PhysicalKeys.text(10, true, true, false));
        eq("@", PhysicalKeys.text(9, false, true, false));
        eq(",", PhysicalKeys.text(76, true, true, false));
        eq(null, PhysicalKeys.text(67, true, false, false));
        eq(null, PhysicalKeys.text(66, true, false, false));
        System.out.println("13 physical keyboard mapping checks passed");
    }
}
