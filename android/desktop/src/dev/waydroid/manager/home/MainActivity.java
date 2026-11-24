// SPDX-License-Identifier: GPL-3.0-or-later
package dev.waydroid.manager.home;

import android.app.Activity;
import android.app.AlertDialog;
import android.app.WallpaperManager;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.SharedPreferences;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.content.res.Configuration;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.RectF;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.net.Uri;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.provider.Settings;
import android.text.Editable;
import android.text.TextWatcher;
import android.text.TextUtils;
import android.view.Gravity;
import android.view.View;
import android.view.WindowManager;
import android.view.inputmethod.InputMethodManager;
import android.view.inputmethod.EditorInfo;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.widget.GridLayout;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import android.widget.Toast;
import org.json.JSONArray;
import java.io.InputStream;
import java.text.Collator;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public final class MainActivity extends Activity {
    private static final int INK = Color.rgb(234, 239, 232);
    private static final int MUTED = Color.rgb(155, 169, 157);
    private static final int PICK_WALLPAPER = 50;
    private final ExecutorService worker = Executors.newSingleThreadExecutor();
    private final Handler handler = new Handler(Looper.getMainLooper());
    private final List<App> apps = new ArrayList<>();
    private SharedPreferences prefs;
    private GridLayout grid;
    private EditText search;
    private Background background;
    private boolean destroyed;
    private int catalogGeneration;

    private static final class App {
        String pkg, label;
        Drawable icon;
        Intent launch;
        boolean enabled;
    }

    private final BroadcastReceiver packages = new BroadcastReceiver() {
        @Override public void onReceive(Context context, Intent intent) { refresh(); }
    };
    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        prefs = getSharedPreferences("desktop", MODE_PRIVATE);
        getWindow().setStatusBarColor(Color.rgb(22, 27, 25));
        getWindow().setNavigationBarColor(Color.rgb(22, 27, 25));
        getWindow().setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);
        build();
        IntentFilter filter = new IntentFilter();
        for (String action : new String[]{Intent.ACTION_PACKAGE_ADDED, Intent.ACTION_PACKAGE_REMOVED,
                Intent.ACTION_PACKAGE_REPLACED, Intent.ACTION_PACKAGE_CHANGED}) filter.addAction(action);
        filter.addDataScheme("package");
        registerReceiver(packages, filter, Context.RECEIVER_EXPORTED);
        loadWallpaper();
        handleAction(getIntent());
    }

    @Override protected void onResume() { super.onResume(); refresh(); }
    @Override protected void onNewIntent(Intent intent) { super.onNewIntent(intent); handleAction(intent); refresh(); }
    private void handleAction(Intent intent) {
        String action = intent.getStringExtra("desktop_action");
        if ("wallpaper".equals(action)) wallpapers();
        else if ("settings".equals(action)) settings();
        else if ("search".equals(action)) {
            search.setVisibility(View.VISIBLE); search.requestFocus();
        } else { search.setText(""); search.setVisibility(View.GONE); }
    }
    @Override public void onConfigurationChanged(Configuration configuration) {
        super.onConfigurationChanged(configuration);
        grid.post(this::render);
    }
    @Override protected void onDestroy() {
        destroyed = true;
        unregisterReceiver(packages);
        handler.removeCallbacksAndMessages(null);
        worker.shutdownNow();
        super.onDestroy();
    }
    @Override public void onBackPressed() { search.setText(""); search.setVisibility(View.GONE); }

    private int dp(float value) { return Math.round(value * getResources().getDisplayMetrics().density); }
    private GradientDrawable panel(int color, int radius) {
        GradientDrawable shape = new GradientDrawable();
        shape.setColor(color); shape.setCornerRadius(dp(radius)); return shape;
    }
    private TextView text(String value, int size, int color) {
        TextView view = new TextView(this); view.setText(value); view.setTextSize(size); view.setTextColor(color);
        view.setFontFeatureSettings("kern"); return view;
    }
    private void build() {
        FrameLayout root = new FrameLayout(this);
        background = new Background(this); root.addView(background, new FrameLayout.LayoutParams(-1, -1));
        LinearLayout layout = new LinearLayout(this); layout.setOrientation(LinearLayout.VERTICAL);
        layout.setPadding(dp(22), dp(28), dp(22), dp(18));
        root.addView(layout, new FrameLayout.LayoutParams(-1, -1));
        search = new EditText(this); search.setSingleLine(true); search.setTextSize(14);
        search.setTextColor(INK); search.setHintTextColor(MUTED); search.setHint("Поиск приложений");
        search.setShowSoftInputOnFocus(false);
        search.setImeOptions(EditorInfo.IME_ACTION_DONE);
        search.setOnEditorActionListener((view, action, event) -> {
            if (action != EditorInfo.IME_ACTION_DONE) return false;
            ((InputMethodManager) getSystemService(INPUT_METHOD_SERVICE)).hideSoftInputFromWindow(search.getWindowToken(), 0);
            search.clearFocus(); return true;
        });
        search.setPadding(dp(18), 0, dp(18), 0); search.setBackground(panel(0x85252e29, 18));
        search.addTextChangedListener(new TextWatcher() {
            public void beforeTextChanged(CharSequence s, int start, int count, int after) {}
            public void onTextChanged(CharSequence s, int start, int before, int count) { render(); }
            public void afterTextChanged(Editable s) {}
        });
        search.setVisibility(View.GONE);
        LinearLayout.LayoutParams searchParams = new LinearLayout.LayoutParams(dp(300), dp(42));
        searchParams.bottomMargin = dp(12); layout.addView(search, searchParams);
        ScrollView scroll = new ScrollView(this); scroll.setFillViewport(true); scroll.setClipToPadding(false);
        grid = new GridLayout(this); grid.setPadding(0, dp(5), 0, dp(14)); scroll.addView(grid);
        layout.addView(scroll, new LinearLayout.LayoutParams(-1, 0, 1));
        setContentView(root);
        grid.addOnLayoutChangeListener((v,l,t,r,b,ol,ot,or,ob) -> {
            if (r-l != or-ol && r-l > 0) grid.post(this::render);
        });
    }

    private void refresh() {
        final int generation = ++catalogGeneration;
        worker.submit(() -> {
            PackageManager pm = getPackageManager();
            Map<String, App> found = new HashMap<>();
            List<ApplicationInfo> installed = pm.getInstalledApplications(0);
            Collator collator = Collator.getInstance();
            installed.sort((a,b) -> collator.compare(a.loadLabel(pm).toString(), b.loadLabel(pm).toString()));
            List<String> ids = new ArrayList<>();
            for (ApplicationInfo info : installed) {
                Intent launch = pm.getLaunchIntentForPackage(info.packageName);
                if (launch == null) launch = pm.getLeanbackLaunchIntentForPackage(info.packageName);
                boolean user = (info.flags & ApplicationInfo.FLAG_SYSTEM) == 0
                        || (info.flags & ApplicationInfo.FLAG_UPDATED_SYSTEM_APP) != 0;
                if (!AppCatalog.include(getPackageName(), info.packageName, user, launch != null)) continue;
                App app = new App(); app.pkg = info.packageName; app.label = info.loadLabel(pm).toString();
                app.icon = info.loadIcon(pm); app.launch = launch; app.enabled = info.enabled;
                if (AppCatalog.PLAY.equals(app.pkg)) app.label = "Play Маркет";
                ids.add(app.pkg); found.put(app.pkg, app);
            }
            List<String> old = new ArrayList<>();
            try {
                JSONArray saved = new JSONArray(prefs.getString("order", "[]"));
                for (int i=0; i<saved.length(); i++) old.add(saved.getString(i));
            } catch (Exception ignored) { /* corrupted preference: rebuild from installed packages */ }
            List<String> order = AppCatalog.order(ids, old);
            List<App> next = new ArrayList<>();
            for (String pkg : order) {
                App app = found.get(pkg);
                if (app == null) {
                    app = new App(); app.pkg = AppCatalog.PLAY; app.label = "Play Маркет";
                    app.icon = pm.getDefaultActivityIcon(); app.enabled = false;
                }
                next.add(app);
            }
            handler.post(() -> {
                if (destroyed || generation != catalogGeneration) return;
                prefs.edit().putString("order", new JSONArray(order).toString()).apply();
                apps.clear(); apps.addAll(next); render();
            });
        });
    }

    private void render() {
        if (grid == null || grid.getWidth() <= 0) return;
        grid.removeAllViews();
        int columns = Math.max(2, Math.min(8, grid.getWidth() / dp(155)));
        grid.setColumnCount(columns);
        String query = search.getText().toString().trim().toLowerCase(Locale.getDefault());
        int cellWidth = grid.getWidth() / columns;
        int visible = 0;
        for (App app : apps) {
            if (!query.isEmpty() && !app.label.toLowerCase(Locale.getDefault()).contains(query)
                    && !app.pkg.toLowerCase(Locale.ROOT).contains(query)) continue;
            LinearLayout tile = new LinearLayout(this); tile.setOrientation(LinearLayout.VERTICAL);
            tile.setGravity(Gravity.TOP | Gravity.CENTER_HORIZONTAL); tile.setPadding(dp(8), dp(8), dp(8), dp(8));
            tile.setFocusable(true); tile.setClickable(true);
            ImageView icon = new ImageView(this); icon.setImageDrawable(app.icon);
            tile.addView(icon, new LinearLayout.LayoutParams(dp(48), dp(48)));
            TextView label = text(app.label, 13, INK); label.setGravity(Gravity.CENTER);
            label.setMaxLines(2); label.setEllipsize(TextUtils.TruncateAt.END); label.setShadowLayer(dp(2), 0, dp(1), Color.BLACK);
            LinearLayout.LayoutParams labelParams = new LinearLayout.LayoutParams(-1, dp(36)); labelParams.topMargin = dp(8);
            tile.addView(label, labelParams);
            tile.setContentDescription(app.label + " · " + app.pkg);
            tile.setOnClickListener(v -> launch(app));
            tile.setOnLongClickListener(v -> { appDetails(app); return true; });
            GridLayout.LayoutParams params = new GridLayout.LayoutParams();
            params.width = cellWidth; params.height = dp(112);
            params.setMargins(0, 0, 0, dp(14)); grid.addView(tile, params); visible++;
        }
        if (visible == 0) {
            TextView empty = text("Ничего не найдено. Попробуйте другое название.", 15, MUTED);
            GridLayout.LayoutParams p = new GridLayout.LayoutParams(); p.columnSpec = GridLayout.spec(0, columns);
            p.width = -1; p.setMargins(dp(6), dp(30), 0, 0); grid.addView(empty, p);
        }
    }
    private void launch(App app) {
        if (!app.enabled || app.launch == null) {
            new AlertDialog.Builder(this).setTitle(app.label)
                    .setMessage("У этого пакета нет доступного окна запуска. Значок сохранён на рабочем столе; можно открыть сведения о приложении.")
                    .setPositiveButton("Сведения", (d,w) -> appDetails(app)).setNegativeButton("Закрыть", null).show();
            return;
        }
        ((InputMethodManager) getSystemService(INPUT_METHOD_SERVICE)).hideSoftInputFromWindow(search.getWindowToken(), 0);
        try { startActivity(app.launch); }
        catch (Exception error) { toast("Приложение изменилось. Обновляем рабочий стол."); refresh(); }
    }
    private void appDetails(App app) { open(new Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:" + app.pkg))); }
    private void open(Intent intent) {
        try { startActivity(intent); } catch (Exception error) { toast("Это действие недоступно в текущем Android."); }
    }
    private void toast(String message) { Toast.makeText(this, message, Toast.LENGTH_LONG).show(); }
    private void settings() {
        String[] names = {"Обои рабочего стола", "Экран и размер элементов", "Клавиатура и язык", "Звук", "Сеть Android", "Все приложения Android"};
        new AlertDialog.Builder(this).setTitle("Параметры рабочего стола").setItems(names, (dialog, index) -> {
            switch (index) {
                case 0: wallpapers(); break;
                case 1: open(new Intent(Settings.ACTION_DISPLAY_SETTINGS)); break;
                case 2: open(new Intent(Settings.ACTION_INPUT_METHOD_SETTINGS)); break;
                case 3: open(new Intent(Settings.ACTION_SOUND_SETTINGS)); break;
                case 4: open(new Intent(Settings.ACTION_WIRELESS_SETTINGS)); break;
                case 5: open(new Intent(Settings.ACTION_MANAGE_APPLICATIONS_SETTINGS)); break;
                default: break;
            }
        }).setNegativeButton("Закрыть", null).show();
    }
    private void wallpapers() {
        new AlertDialog.Builder(this).setTitle("Обои").setItems(new String[]{"Серо-зелёные · авторские", "Выбрать своё изображение"}, (dialog, index) -> {
            if (index == 0) { prefs.edit().remove("wallpaper").apply(); loadWallpaper(); }
            else {
                Intent pick = new Intent(Intent.ACTION_OPEN_DOCUMENT).setType("image/*").addCategory(Intent.CATEGORY_OPENABLE);
                pick.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
                try { startActivityForResult(pick, PICK_WALLPAPER); } catch (Exception error) { toast("Выбор изображений недоступен."); }
            }
        }).setNegativeButton("Закрыть", null).show();
    }
    @Override protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request == PICK_WALLPAPER && result == RESULT_OK && data != null && data.getData() != null) {
            Uri uri = data.getData();
            try { getContentResolver().takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION); }
            catch (SecurityException ignored) { /* current grant may still allow this session */ }
            prefs.edit().putString("wallpaper", uri.toString()).apply(); loadWallpaper();
        }
    }
    private InputStream wallpaperStream(String uri) throws Exception {
        return uri.isEmpty() ? getAssets().open("wallpaper-gray-green.png") : getContentResolver().openInputStream(Uri.parse(uri));
    }
    private void loadWallpaper() {
        final String uri = prefs.getString("wallpaper", "");
        worker.submit(() -> {
            try {
                BitmapFactory.Options bounds = new BitmapFactory.Options(); bounds.inJustDecodeBounds = true;
                try (InputStream stream = wallpaperStream(uri)) { BitmapFactory.decodeStream(stream, null, bounds); }
                BitmapFactory.Options options = new BitmapFactory.Options(); options.inSampleSize = 1;
                while (Math.max(bounds.outWidth, bounds.outHeight) / options.inSampleSize > 2560) options.inSampleSize *= 2;
                Bitmap bitmap;
                try (InputStream stream = wallpaperStream(uri)) { bitmap = BitmapFactory.decodeStream(stream, null, options); }
                if (bitmap == null) throw new IllegalArgumentException("Not an image");
                handler.post(() -> { if (!destroyed) { background.bitmap = bitmap; background.invalidate(); } });
                // Match the real Android wallpaper as well as this launcher's background.
                if (!prefs.getString("appliedWallpaper", "none").equals(uri)) {
                    try {
                        WallpaperManager.getInstance(this).setBitmap(bitmap);
                        prefs.edit().putString("appliedWallpaper", uri).apply();
                    } catch (Exception ignored) { /* launcher background still works without system wallpaper service */ }
                }
            } catch (Exception error) {
                handler.post(() -> {
                    if (destroyed) return;
                    toast("Не удалось открыть обои. Возвращаем серо-зелёный фон.");
                    if (!uri.isEmpty()) { prefs.edit().remove("wallpaper").apply(); loadWallpaper(); }
                });
            }
        });
    }
    private static final class Background extends View {
        Bitmap bitmap;
        final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG | Paint.FILTER_BITMAP_FLAG);
        Background(Context context) { super(context); }
        @Override protected void onDraw(Canvas canvas) {
            canvas.drawColor(Color.rgb(29, 36, 31));
            if (bitmap != null) {
                float scale = Math.max(getWidth() / (float) bitmap.getWidth(), getHeight() / (float) bitmap.getHeight());
                float w = bitmap.getWidth() * scale, h = bitmap.getHeight() * scale;
                canvas.drawBitmap(bitmap, null, new RectF((getWidth()-w)/2, (getHeight()-h)/2, (getWidth()+w)/2, (getHeight()+h)/2), paint);
            }
            canvas.drawColor(0x18202720);
        }
    }
}
