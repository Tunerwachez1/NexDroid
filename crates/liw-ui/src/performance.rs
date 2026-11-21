//! Native performance settings and extensible JSON translations.
use std::{collections::BTreeMap, path::PathBuf};
use gpui::{prelude::*, *};
use serde::{Deserialize, Serialize};
use crate::theme::Theme;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub cpus: u32,
    pub memory_mib: u32,
    pub fps: u32,
    pub show_fps: bool,
    pub high_fps: bool,
    pub language: String,
    pub fullscreen_start: bool,
    pub escape_fullscreen: bool,
    pub escape_mouse: bool,
    pub screenshots: Option<PathBuf>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self { cpus: 0, memory_mib: 0, fps: 240, show_fps: false, high_fps: true, language: "ru".into(),
            fullscreen_start:false, escape_fullscreen:true, escape_mouse:true, screenshots:None }
    }
}
pub fn directory() -> PathBuf { liw_core::Config::path().parent().unwrap().to_owned() }
impl Preferences {
    pub fn screenshot_directory(&self) -> PathBuf {
        self.screenshots.clone().unwrap_or_else(|| {
            std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
                .join("Pictures").join("NexDroid")
        })
    }
    fn save_general_in(&self, dir: &std::path::Path) -> anyhow::Result<()> {
        if let Some(path)=&self.screenshots { anyhow::ensure!(path.is_absolute(), "Screenshot directory must be absolute"); }
        std::fs::create_dir_all(dir)?;
        let path=dir.join("performance.json");
        let mut value=match std::fs::read_to_string(&path) {
            Ok(s)=>serde_json::from_str::<serde_json::Value>(&s)?,
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>serde_json::to_value(Self::default())?,
            Err(e)=>return Err(e.into()),
        };
        anyhow::ensure!(value.is_object(), "performance.json must be an object");
        let draft=serde_json::to_value(self)?;
        for key in ["language","fullscreen_start","escape_fullscreen","escape_mouse","screenshots"] { value[key]=draft[key].clone(); }
        atomic_json(&path,&value)
    }
    pub fn load() -> Self {
        let mut p: Self = std::fs::read_to_string(directory().join("performance.json"))
            .ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        if let Ok(s) = std::fs::read_to_string(directory().join("display.json")) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                p.fps = v["fps"].as_u64().unwrap_or(p.fps as u64) as u32;
                p.cpus = v["cpus"].as_u64().unwrap_or(p.cpus as u64) as u32;
                p.memory_mib = v["memory_mib"].as_u64().unwrap_or(p.memory_mib as u64) as u32;
            }
        }
        // Older display profiles can already request more than 60 Hz.
        if p.fps > 60 { p.high_fps = true; }
        p
    }
    fn save(&self) -> anyhow::Result<()> {
        self.save_in(&directory())
    }
    fn save_in(&self, dir: &std::path::Path) -> anyhow::Result<()> {
        anyhow::ensure!((24..=360).contains(&self.fps), "FPS must be 24–360");
        anyhow::ensure!(self.cpus <= cpu_count(), "CPU count exceeds available CPUs");
        anyhow::ensure!(self.memory_mib == 0 || (1024..=memory_limit()).contains(&self.memory_mib), "RAM limit exceeds available memory");
        std::fs::create_dir_all(dir)?;
        // Retain DPI, resolution and any fields owned by other settings pages.
        let display = dir.join("display.json");
        let mut value = match std::fs::read_to_string(&display) {
            Ok(s) => serde_json::from_str::<serde_json::Value>(&s)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::to_value(liw_core::display::GuestDisplay::load().map_err(anyhow::Error::msg)?)?,
            Err(e) => return Err(e.into()),
        };
        anyhow::ensure!(value.is_object(), "display.json must be an object");
        value["fps"] = self.fps.into();
        value["cpus"] = self.cpus.into();
        value["memory_mib"] = self.memory_mib.into();
        atomic_json(&display, &value)?;
        atomic_json(&dir.join("performance.json"), self)
    }
}
pub(super) fn atomic_json(path: &std::path::Path, value: &impl Serialize) -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}
fn cpu_count() -> u32 { std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1) }
fn memory_limit() -> u32 {
    std::fs::read_to_string("/proc/meminfo").ok().and_then(|s| {
        s.lines().find(|l| l.starts_with("MemTotal:")).and_then(|l| l.split_whitespace().nth(1))
            .and_then(|n| n.parse::<u32>().ok()).map(|n| n / 1024)
    }).unwrap_or(1024)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Language { id: String, name: String, strings: BTreeMap<String, String> }
fn languages() -> Vec<Language> {
    let mut result = vec![serde_json::from_str(include_str!("../locales/ru.json")).unwrap(), serde_json::from_str(include_str!("../locales/en.json")).unwrap()];
    if let Ok(files) = std::fs::read_dir(directory().join("languages")) {
        for entry in files.flatten() {
            if let Ok(language) = read_language(&entry.path()) {
                result.retain(|l: &Language| l.id != language.id);
                result.push(language);
            }
        }
    }
    result
}
fn read_language(path: &std::path::Path) -> anyhow::Result<Language> {
    anyhow::ensure!(std::fs::metadata(path)?.len() <= 1_048_576, "Language file exceeds 1 MiB");
    let language: Language = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    anyhow::ensure!(!language.id.is_empty() && language.id.len() <= 32 && language.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'), "Invalid language ID");
    anyhow::ensure!(!language.name.trim().is_empty(), "Language name is empty");
    Ok(language)
}
pub struct Performance {
    embedded: bool,
    advanced:Option<crate::advanced::State>,
    advanced_draft:crate::advanced::Settings,
    probing_advanced:bool,
    root_draft:bool,
    shortcuts:crate::shortcuts::Shortcuts,
    shortcuts_error:String,
    recording:Option<crate::shortcuts::Action>,
    profile_capture:Option<u16>,
    physical_shortcuts:bool,
    pending_physical:Option<crate::shortcuts::Action>,
    shortcut_modifiers:std::collections::BTreeSet<u16>,
    phone_id:String,
    phone_loaded:bool,
    probing_phone:bool,
    active_phone:String,
    about_info: Option<crate::about::AboutInfo>,
    probing_about: bool,
    devices: liw_core::devices::DeviceSettings,
    device_inventory: liw_core::devices::Inventory,
    device_error: String,
    probing_devices: bool,
    graphics: liw_core::graphics::GraphicsSettings,
    gpus: Vec<liw_core::graphics::Gpu>,
    android_graphics: Option<liw_core::graphics::AndroidGraphics>,
    graphics_error: String,
    probing_graphics: bool,
    tab: &'static str,
    display: Option<crate::display_settings::DisplaySettings>,
    custom_resolution: bool,
    focus: FocusHandle,
    editing: Option<&'static str>,
    number: String,
    replace_number: bool,
    draft: Preferences,
    languages: Vec<Language>,
    message: String,
    busy: bool,
    menu: Option<&'static str>,
    slider_bounds: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    dragging: bool,
}
impl Performance {
    fn new(cx: &mut Context<Self>) -> Self {
        let result = crate::display_settings::DisplaySettings::load();
        let message = result.as_ref().err().map(ToString::to_string).unwrap_or_default();
        let display = result.ok();
        let custom_resolution = display.as_ref().is_some_and(|d| d.preset_index().is_none());
        let graphics = liw_core::graphics::GraphicsSettings::load();
        let graphics_error = graphics.as_ref().err().cloned().unwrap_or_default();
        let devices = liw_core::devices::DeviceSettings::load();
        let device_error = devices.as_ref().err().cloned().unwrap_or_default();
        let shortcuts=crate::shortcuts::Shortcuts::load();
        let shortcuts_error=shortcuts.as_ref().err().map(ToString::to_string).unwrap_or_default();
        Self { embedded:false,advanced:None,advanced_draft:Default::default(),probing_advanced:false,root_draft:false,shortcuts:shortcuts.unwrap_or_default(),shortcuts_error,recording:None,profile_capture:None,physical_shortcuts:false,pending_physical:None,shortcut_modifiers:Default::default(), phone_id:"auto".into(),phone_loaded:false,probing_phone:false,active_phone:String::new(), about_info:None, probing_about:false, devices: devices.unwrap_or_default(), device_inventory: Default::default(), device_error, probing_devices: false,
            graphics: graphics.unwrap_or_default(), gpus: liw_core::graphics::discover(), android_graphics: None, graphics_error, probing_graphics: false,
            tab: "performance", display, custom_resolution, focus: cx.focus_handle(), editing: None, number: String::new(), replace_number: false,
            draft: Preferences::load(), languages: languages(), message, busy: false, menu: None, slider_bounds: Default::default(), dragging: false }
    }
    pub fn embedded(cx: &mut Context<Self>) -> Self {
        let mut view=Self::new(cx);
        view.embedded=true;
        view
    }
    pub fn focus(&self,window: &mut Window,cx: &mut App) { window.focus(&self.focus,cx); }
    pub fn set_capture_key(&mut self,key:Option<u16>) {self.profile_capture=key;}
    pub fn use_physical_shortcuts(&mut self,enabled:bool) {self.physical_shortcuts=enabled;if !enabled {self.pending_physical=None;}self.shortcut_modifiers.clear();}
    pub fn physical_shortcut_key(&mut self,key:u16,pressed:bool,cx:&mut Context<Self>) {
        use crate::shortcuts::{Chord,CTRL,SHIFT,ALT,SUPER};
        if !pressed {self.shortcut_modifiers.remove(&key);return;}
        self.shortcut_modifiers.insert(key);
        let Some(action)=self.pending_physical else {return;};
        // Esc is cancelled by the focused GPUI handler, avoiding a second
        // delivery closing the entire settings form.
        if key==1 {return;}
        let held=|keys:&[u16]|keys.iter().any(|k|self.shortcut_modifiers.contains(k));
        let chord=Chord {key,modifiers:u8::from(held(&[29,97]))*CTRL | u8::from(held(&[42,54]))*SHIFT | u8::from(held(&[56,100]))*ALT | u8::from(held(&[125,126]))*SUPER};
        if chord.valid() {
            let b=self.shortcuts.bindings.get_mut(&action).unwrap();b.chord=Some(chord);b.enabled=true;
            self.recording=None;self.pending_physical=None;self.message.clear();cx.notify();
        }
    }
    fn close(&mut self,window: &mut Window,cx: &mut Context<Self>) {
        if self.embedded { cx.emit(SettingsClosed); }
        else { window.remove_window(); }
        cx.stop_propagation();
    }
    fn numeric_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(field) = self.editing else { return; };
        let key = event.keystroke.key.as_str();
        match key {
            "escape" => self.editing = None,
            "enter" => {
                if let Ok(value) = self.number.parse::<u32>() {
                    let valid = if field == "density" { value == 0 || (120..=640).contains(&value) } else { (320..=7680).contains(&value) };
                    if valid {
                        if let Some(d) = &mut self.display {
                            match field { "width" => d.width = value, "height" => d.height = value, _ => d.dpi = value }
                        }
                        self.editing = None;
                    } else { self.message = self.text("number_range").to_string(); }
                }
            }
            "backspace" => { if self.replace_number { self.number.clear(); } else { self.number.pop(); } self.replace_number = false; }
            _ if key.len() == 1 && key.as_bytes()[0].is_ascii_digit() => {
                if self.replace_number { self.number.clear(); self.replace_number = false; }
                if self.number.len() < 4 { self.number.push_str(key); }
            }
            _ => {}
        }
        cx.stop_propagation();
        cx.notify();
    }
    fn refresh_about(&mut self,cx: &mut Context<Self>) {
        if self.probing_about { return; }
        self.probing_about=true;
        cx.spawn(async move |view,cx| {
            let result=gpui_tokio::Tokio::spawn(cx,crate::about::read()).await;
            let _=view.update(cx,|v,cx| {
                v.probing_about=false;
                match result { Ok(info)=>v.about_info=Some(info),Err(e)=>v.message=e.to_string() }
                cx.notify();
            });
        }).detach();
    }
    fn refresh_phone(&mut self,cx: &mut Context<Self>) {
        if self.probing_phone || self.busy {return;}
        self.probing_phone=true;self.phone_loaded=false;
        cx.spawn(async move |view,cx| {
            let result=gpui_tokio::Tokio::spawn(cx,async {
                let helper=liw_core::HelperClient::connect().await.map_err(|e|e.to_string())?;
                let id=helper.phone_profile().await.map_err(|e|e.to_string())?;
                if !liw_core::phone::valid(&id) {return Err("Unknown saved phone profile".to_string());}
                let model=helper.get_prop("ro.product.model").await.unwrap_or_default();
                Ok((id,model))
            }).await;
            let _=view.update(cx,|v,cx| {
                v.probing_phone=false;
                match result {
                    Ok(Ok((id,model)))=>{v.phone_id=id;v.active_phone=model;v.phone_loaded=true;},
                    Ok(Err(e))=>v.message=e,
                    Err(e)=>v.message=e.to_string(),
                }
                cx.notify();
            });
        }).detach();
    }
    fn refresh_advanced(&mut self,cx:&mut Context<Self>) {
        if self.probing_advanced || self.busy {return;}
        self.probing_advanced=true;self.advanced=None;
        cx.spawn(async move |view,cx| {
            let result=gpui_tokio::Tokio::spawn(cx,crate::advanced::read()).await;
            let _=view.update(cx,|v,cx| {
                v.probing_advanced=false;
                match result {Ok(Ok(state))=>{v.advanced_draft=state.settings;v.root_draft=state.root.requested;v.advanced=Some(state);v.message.clear();},Ok(Err(e))=>v.message=e,Err(e)=>v.message=e.to_string()}
                cx.notify();
            });
        }).detach();
    }
    fn advanced_content(&self,t:&Theme,cx:&mut Context<Self>)->AnyElement {
        let hint=|key|div().whitespace_normal().text_size(px(12.)).text_color(t.text_muted).child(self.text(key));
        let row=|key:&'static str,on:bool|div().id(key).flex().justify_between().items_center().gap(px(18.)).py(px(12.)).cursor_pointer()
            .child(div().flex_1().whitespace_normal().child(self.text(key))).child(toggle(on,t))
            .on_click(cx.listener(move |v,_,_,cx| {
                if v.busy || v.probing_advanced {return;}
                let Some(state)=&v.advanced else {return;};
                if key=="advanced_root" {
                    if !state.root.available {v.message=v.text("root_unavailable").to_string();cx.notify();return;}
                    v.root_draft=!v.root_draft;cx.notify();return;
                }
                if key=="advanced_adb" && !v.advanced_draft.adb && !state.adb_secure {v.message=v.text("advanced_insecure").to_string();cx.notify();return;}
                let field=match key {"advanced_adb"=>&mut v.advanced_draft.adb,"advanced_touches"=>&mut v.advanced_draft.show_touches,_=>&mut v.advanced_draft.pointer_location};
                *field=!*field;cx.notify();
            }));
        let mut body=div().w_full().max_w(px(580.)).flex().flex_col().gap(px(14.))
            .child(self.text("advanced_abi"));
        if let Some(state)=&self.advanced {
            body=body.child(div().whitespace_normal().text_color(t.text_muted).child(SharedString::from(state.abis.clone())))
                .child(hint("advanced_abi_help"))
                .child(div().h(px(8.)).border_b_1().border_color(t.border))
                .child(row("advanced_root",self.root_draft))
                .child(hint("root_help"))
                .child(div().whitespace_normal().text_color(t.text_muted).child(SharedString::from(format!("{}: {}",self.text("root_current"),self.text(if state.root.active {"advanced_on"} else {"advanced_off"})))))
                .when(state.root.pending,|s|s.child(hint("root_pending")))
                .when(!state.root.available,|s|s.child(hint("root_unavailable")))
                .when(state.root.active && !state.root.manager_package.is_empty(),|s|s.child(button(self.text("root_manager"),"open-root-manager",false,t).on_click(cx.listener(|v,_,_,cx| {
                    let Some(state)=&v.advanced else {return;};let package=state.root.manager_package.clone();
                    cx.spawn(async move |view,cx| {
                        let result=gpui_tokio::Tokio::spawn(cx,async move {
                            let manager=liw_core::Manager::connect().await.map_err(|e|e.to_string())?;
                            manager.launch(&package).await.map_err(|e|e.to_string())
                        }).await;
                        let _=view.update(cx,|v,cx| {match result {Ok(Ok(()))=>cx.emit(SettingsClosed),Ok(Err(e))=>v.message=e,Err(e)=>v.message=e.to_string()};cx.notify();});
                    }).detach();
                }))))
                .child(div().h(px(8.)).border_b_1().border_color(t.border))
                .child(row("advanced_adb",self.advanced_draft.adb))
                .child(hint("advanced_adb_help"))
                .when(!state.adb_secure,|s|s.child(hint("advanced_insecure")))
                .child(div().whitespace_normal().text_color(t.text_muted).child(SharedString::from(format!("{}: {}",self.text("advanced_current_adb"),self.text(if state.settings.adb {"advanced_on"} else {"advanced_off"})))))
                .when(state.settings.adb && !state.adb_address.is_empty(),|s|s.child(div().flex().flex_wrap().gap(px(10.))
                    .child(SharedString::from(format!("adb connect {}",state.adb_address)))
                    .child(button(self.text("advanced_copy"),"copy-adb-address",false,t).on_click(cx.listener(|v,_,_,cx| {
                        if let Some(state)=&v.advanced {cx.write_to_clipboard(ClipboardItem::new_string(format!("adb connect {}",state.adb_address)));}
                    })))))
                .child(div().h(px(8.)).border_b_1().border_color(t.border))
                .child(self.text("advanced_input"))
                .child(row("advanced_touches",self.advanced_draft.show_touches))
                .child(row("advanced_pointer",self.advanced_draft.pointer_location))
                .child(hint("advanced_input_help"));
        } else {body=body.child(hint(if self.probing_advanced {"advanced_loading"} else {"advanced_unavailable"}));}
        div().id("advanced-scroll").flex_1().min_h_0().min_w_0().overflow_y_scroll().px(px(30.)).py(px(22.))
            .child(body.child(button(self.text("advanced_refresh"),"refresh-advanced",false,t).on_click(cx.listener(|v,_,_,cx|v.refresh_advanced(cx)))))
            .into_any_element()
    }
    fn phone_content(&self,t:&Theme,cx:&mut Context<Self>) -> AnyElement {
        let mut options=vec![(0,self.text("phone_original"))];
        options.extend(liw_core::phone::PROFILES.iter().enumerate().map(|(i,p)|(i as u32+1,format!("Google {}",p.model).into())));
        let selection=liw_core::phone::PROFILES.iter().position(|p|p.id==self.phone_id).map(|i|i as u32+1).unwrap_or(0);
        let profile=liw_core::phone::find(&self.phone_id);
        let line=|key,value:SharedString|div().flex().flex_wrap().gap(px(12.))
            .child(div().w(px(150.)).text_color(t.text_muted).child(self.text(key)))
            .child(div().flex_1().min_w(px(100.)).whitespace_normal().child(value));
        let hint=|key|div().whitespace_normal().text_size(px(12.)).text_color(t.text_muted).child(self.text(key));
        div().id("phone-scroll").flex_1().min_h_0().min_w_0().overflow_y_scroll().px(px(30.)).py(px(22.))
            .child(div().w_full().max_w(px(580.)).flex().flex_col().gap(px(16.))
                .child(self.text("phone_profile"))
                .when(self.phone_loaded,|s|s.child(self.combo("phone_profile",selection,options,t,cx)))
                .when(!self.phone_loaded,|s|s.child(hint(if self.probing_phone {"phone_loading"} else {"phone_unavailable"})))
                .when_some(profile,|s,p|s
                    .child(line("phone_manufacturer","Google".into()))
                    .child(line("phone_model",p.model.into()))
                    .child(line("phone_device",p.device.into())))
                .child(hint("phone_help"))
                .child(div().h(px(8.)).border_b_1().border_color(t.border))
                .child(line("phone_active",if self.active_phone.is_empty() {self.text("unknown")} else {self.active_phone.clone().into()}))
                .child(hint("phone_restart"))
                .child(button(self.text("phone_refresh"),"refresh-phone",false,t).on_click(cx.listener(|v,_,_,cx|v.refresh_phone(cx)))))
            .into_any_element()
    }
    fn about_content(&self,t: &Theme,cx: &mut Context<Self>) -> AnyElement {
        let info=self.about_info.as_ref();
        let version=|value: Option<&String>| SharedString::from(value.cloned().unwrap_or_else(||self.text("unknown").to_string()));
        let line=|label: SharedString,value: SharedString| div().flex().flex_wrap().gap(px(12.))
            .child(div().w(px(130.)).text_color(t.text_muted).child(label))
            .child(div().flex_1().min_w(px(120.)).whitespace_normal().child(value));
        let link=|id: &'static str,label: SharedString,url: &'static str| button(label,id,false,t)
            .on_click(move |_,_,cx|cx.open_url(url));
        div().id("about-scroll").flex_1().min_h_0().min_w_0().overflow_y_scroll().px(px(30.)).py(px(22.))
            .child(div().w_full().max_w(px(580.)).flex().flex_col().gap(px(16.))
                .child(div().flex().items_center().gap(px(14.))
                    .child(div().size(px(48.)).rounded(px(12.)).bg(t.accent).flex().items_center().justify_center().text_size(px(28.)).child("N"))
                    .child(div().flex().flex_col().gap(px(4.))
                        .child(div().text_size(px(24.)).font_weight(FontWeight::SEMIBOLD).child("NexDroid"))
                        .child(SharedString::from(format!("{} · {}",env!("CARGO_PKG_VERSION"),self.text("beta_status"))))))
                .child(div().whitespace_normal().text_color(t.text_muted).child(self.text("about_description")))
                .child(div().h(px(6.)).border_b_1().border_color(t.border))
                .child(line(self.text("version_label"),SharedString::from(env!("CARGO_PKG_VERSION"))))
                .child(line("Waydroid".into(),version(info.and_then(|i|i.waydroid.as_ref()))))
                .child(line("Android".into(),version(info.and_then(|i|i.android.as_ref()))))
                .child(line("Android SDK".into(),version(info.and_then(|i|i.sdk.as_ref()))))
                .child(line("Android ABI".into(),version(info.and_then(|i|i.abis.as_ref()))))
                .child(line(self.text("platform_label"),SharedString::from(format!("{} · {}",std::env::consts::OS,std::env::consts::ARCH))))
                .when(info.is_some_and(|i|i.android.is_none()),|s|s.child(div().whitespace_normal().text_color(t.text_muted).child(self.text("about_android_offline"))))
                .child(button(self.text(if self.probing_about {"about_loading"} else {"about_refresh"}),"refresh-about",false,t)
                    .on_click(cx.listener(|v,_,_,cx|v.refresh_about(cx))))
                .child(div().h(px(6.)).border_b_1().border_color(t.border))
                .child(line(self.text("license_label"),SharedString::from(env!("CARGO_PKG_LICENSE"))))
                .child(div().whitespace_normal().text_color(t.text_muted).child(self.text("upstream_credit")))
                .child(div().flex().flex_wrap().gap(px(8.))
                    .child(link("about-project",self.text("project_link"),env!("CARGO_PKG_REPOSITORY")))
                    .child(link("about-issues",self.text("issues_link"),"https://github.com/Tunerwachez1/NexDroid/issues"))
                    .child(link("about-upstream",self.text("upstream_link"),"https://github.com/Liwinux-Project/liwinux"))
                    .child(link("about-license",self.text("license_link"),"https://github.com/Tunerwachez1/NexDroid/blob/master/LICENSE"))))
            .into_any_element()
    }
    fn refresh_devices(&mut self,cx: &mut Context<Self>) {
        if self.probing_devices { return; }
        self.probing_devices = true;
        cx.spawn(async move |view,cx| {
            let result = gpui_tokio::Tokio::spawn(cx,liw_core::devices::inventory()).await;
            let _ = view.update(cx,|v,cx| {
                v.probing_devices = false;
                match result { Ok(inventory) => { v.device_error = inventory.audio_error.clone(); v.device_inventory = inventory; }, Err(e) => v.device_error = e.to_string() }
                cx.notify();
            });
        }).detach();
    }
    fn devices_content(&self,t: &Theme,cx: &mut Context<Self>) -> AnyElement {
        let options = |list: &[liw_core::devices::AudioDevice]| {
            let mut options = vec![(0,self.text("system_default"))];
            options.extend(list.iter().enumerate().map(|(i,d)|((i+1) as u32,SharedString::from(d.name.clone())))); options
        };
        let selected = |list: &[liw_core::devices::AudioDevice],id: &Option<String>| id.as_ref().and_then(|id|list.iter().position(|d| &d.id==id)).map(|i|i as u32+1).unwrap_or(0);
        let mut cameras = vec![(0,self.text("camera_auto")),(1,self.text("camera_off"))];
        cameras.extend(self.device_inventory.cameras.iter().enumerate().map(|(i,c)|((i+2) as u32,SharedString::from(format!("{} · {}",c.name,c.node.display())))));
        let camera = match self.devices.camera.as_str() { "auto" => 0, "off" => 1, id => self.device_inventory.cameras.iter().position(|c|c.id==id).map(|i|i as u32+2).unwrap_or(0) };
        let missing_audio = self.devices.audio_sink.as_ref().is_some_and(|id|!self.device_inventory.sinks.iter().any(|d| &d.id==id))
            || self.devices.audio_source.as_ref().is_some_and(|id|!self.device_inventory.sources.iter().any(|d| &d.id==id));
        let missing_camera = !matches!(self.devices.camera.as_str(),"auto"|"off") && !self.device_inventory.cameras.iter().any(|c| c.id==self.devices.camera);
        let hint = |text: SharedString| div().whitespace_normal().text_size(px(12.)).text_color(t.text_muted).child(text);
        div().id("devices-scroll").flex_1().min_h_0().min_w_0().overflow_y_scroll().px(px(30.)).py(px(22.))
            .child(div().w_full().max_w(px(580.)).flex().flex_col().gap(px(14.))
                .child(self.text("speakers")).child(self.combo("audio_sink",selected(&self.device_inventory.sinks,&self.devices.audio_sink),options(&self.device_inventory.sinks),t,cx))
                .child(self.text("microphone")).child(self.combo("audio_source",selected(&self.device_inventory.sources,&self.devices.audio_source),options(&self.device_inventory.sources),t,cx))
                .child(hint(self.text("audio_help")))
                .when(missing_audio,|s|s.child(hint(self.text("missing_audio"))))
                .child(div().h(px(12.)).border_b_1().border_color(t.border))
                .child(self.text("camera_label")).child(self.combo("camera",camera,cameras,t,cx))
                .child(hint(self.text("camera_help")))
                .when(missing_camera,|s|s.child(hint(self.text("missing_camera"))))
                .when(self.device_inventory.cameras.is_empty(),|s|s.child(hint(self.text("no_cameras"))))
                .child(button(self.text(if self.probing_devices { "loading_devices" } else { "refresh_devices" }),"refresh-devices",false,t)
                    .on_click(cx.listener(|v,_,_,cx|v.refresh_devices(cx))))
                .child(hint(SharedString::from(self.device_error.clone()))))
            .into_any_element()
    }
    fn refresh_graphics(&mut self, cx: &mut Context<Self>) {
        if self.probing_graphics { return; }
        self.gpus = liw_core::graphics::discover();
        self.probing_graphics = true;
        self.graphics_error.clear();
        cx.spawn(async move |view,cx| {
            let result = gpui_tokio::Tokio::spawn(cx, async {
                tokio::time::timeout(std::time::Duration::from_secs(10), async {
                    let helper = liw_core::helper::HelperClient::connect().await.map_err(|e| e.to_string())?;
                    helper.graphics_info().await.map_err(|e| e.to_string())
                }).await.unwrap_or_else(|_| Err("Android graphics probe timed out".into()))
            }).await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = view.update(cx, |v,cx| {
                v.probing_graphics = false;
                match result { Ok(info) => v.android_graphics = Some(info), Err(e) => { v.android_graphics = None; v.graphics_error = e; } }
                cx.notify();
            });
        }).detach();
    }
    fn graphics_content(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let mut options = vec![(0,self.text("auto"))];
        options.extend(self.gpus.iter().enumerate().map(|(i,d)| ((i+1) as u32,SharedString::from(d.label()))));
        let selected = self.graphics.gpu.as_ref().and_then(|id| self.gpus.iter().position(|d| &d.id == id)).map(|i| i as u32+1).unwrap_or(0);
        let value = |text: String| div().w_full().min_w_0().p(px(10.)).bg(t.raised).whitespace_normal().child(SharedString::from(text));
        let hint = |text: SharedString| div().whitespace_normal().text_size(px(12.)).text_color(t.text_muted).child(text);
        let info = self.android_graphics.as_ref();
        let gles = info.map(|i| i.gles.as_str()).filter(|s| !s.is_empty()).map(str::to_owned).unwrap_or_else(|| self.text("unknown").to_string());
        let astc = match info.and_then(|i| i.astc) { Some(true) => "supported", Some(false) => "unsupported", None => "unknown" };
        let chosen = self.graphics.gpu.as_ref().and_then(|id| self.gpus.iter().find(|d| &d.id == id));
        let driver = chosen.map(|d| format!("{} {}",d.driver,d.version)).unwrap_or_else(|| self.text("auto").to_string());
        let missing = self.graphics.gpu.as_ref().filter(|id| !self.gpus.iter().any(|d| &d.id == *id));
        div().id("graphics-scroll").flex_1().min_h_0().min_w_0().overflow_y_scroll().px(px(30.)).py(px(22.))
            .child(div().w_full().max_w(px(580.)).flex().flex_col().gap(px(14.))
                .child(self.text("graphics_gpu")).child(self.combo("gpu",selected,options,t,cx))
                .when_some(missing, |s,id| s.child(hint(SharedString::from(format!("{} {id}",self.text("missing_gpu"))))))
                .child(hint(self.text("gpu_help")))
                .child(self.text("linux_driver")).child(value(driver))
                .children(self.gpus.iter().map(|d| value(format!("{}\n{} {} · {}",d.label(),d.driver,d.version,d.node.display()))))
                .child(self.text("interface_renderer")).child(value(format!("GPUI / WGPU · {}",self.text("auto"))))
                .child(self.text("embedded_renderer")).child(value("OpenGL ES / EGL".into()))
                .child(self.text("android_renderer")).child(value(gles))
                .when_some(info, |s,i| s.child(value(format!("EGL: {} · Vulkan HAL: {}",i.egl,i.vulkan))))
                .child(hint(self.text("api_help")))
                .child(self.text("astc_label")).child(value(format!("{} · {}",self.text("auto"),self.text(astc))))
                .child(hint(self.text("astc_help")))
                .child(button(self.text(if self.probing_graphics { "applying" } else { "refresh_graphics" }), "refresh-graphics", false,t)
                    .on_click(cx.listener(|v,_,_,cx| v.refresh_graphics(cx))))
                .child(hint(SharedString::from(self.graphics_error.clone()))))
            .into_any_element()
    }
    fn numeric(&self, key: &'static str, value: u32, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let text = if self.editing == Some(key) { format!("{}▏",self.number) } else { value.to_string() };
        div().flex().flex_wrap().items_center().gap(px(6.)).child(self.text(key))
            .child(div().id(key).w(px(90.)).h(px(32.)).px(px(10.)).flex().items_center().bg(t.raised).border_1()
                .border_color(if self.editing == Some(key) { t.accent } else { t.border }).cursor_text().child(SharedString::from(text))
                .on_click(cx.listener(move |v, _, window, cx| {
                    v.editing = Some(key); v.number = value.to_string(); v.replace_number = true;
                    window.focus(&v.focus, cx); cx.notify();
                }))).into_any_element()
    }
    fn display_content(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(d) = &self.display else { return div().p(px(30.)).child(SharedString::from(self.message.clone())).into_any_element(); };
        let orientation = vec![(0,self.text("landscape")),(1,self.text("portrait"))];
        let mut resolutions: Vec<_> = crate::display_settings::RESOLUTIONS.iter().enumerate().map(|(i,&(w,h))| {
            let (w,h) = if d.portrait() { (h,w) } else { (w,h) };
            (i as u32, SharedString::from(format!("{w} × {h}")))
        }).collect();
        resolutions.push((6,self.text("custom")));
        let selected = if self.custom_resolution { 6 } else { d.preset_index().unwrap_or(6) as u32 };
        let mut densities = vec![0,160,240,320,400,480,640];
        if !densities.contains(&d.dpi) { densities.push(d.dpi); }
        densities.sort_unstable();
        let densities = densities.into_iter().map(|dpi| (dpi, if dpi == 0 { self.text("auto") } else { format!("{dpi} DPI").into() })).collect();
        div().id("display-scroll").flex_1().min_h_0().min_w_0().overflow_y_scroll().px(px(30.)).py(px(22.))
            .child(div().w_full().max_w(px(580.)).flex().flex_col().gap(px(14.))
                .child(self.text("orientation_label")).child(self.combo("orientation", u32::from(d.portrait()), orientation, t, cx))
                .child(self.text("resolution_label")).child(self.combo("resolution", selected, resolutions, t, cx))
                .when(self.custom_resolution, |s| s.child(div().flex().flex_wrap().gap(px(14.))
                    .child(self.numeric("width",d.width,t,cx)).child(self.numeric("height",d.height,t,cx))))
                .child(div().h(px(12.)).border_b_1().border_color(t.border))
                .child(self.text("density_label")).child(self.combo("dpi", d.dpi, densities, t, cx))
                .child(self.numeric("density", d.dpi, t, cx))
                .child(div().whitespace_normal().text_size(px(12.)).text_color(t.text_muted).child(self.text("number_help")))
                .child(div().h(px(12.)).border_b_1().border_color(t.border))
                .child(SharedString::from(format!("{} {} × {} · {}",self.text("selected_display"),d.width,d.height,
                    if d.dpi == 0 { self.text("auto").to_string() } else { format!("{} DPI",d.dpi) })))
                .child(div().whitespace_normal().text_size(px(12.)).text_color(t.text_muted).child(self.text("display_help"))))
            .into_any_element()
    }
    fn slider_value(&mut self, x: Pixels) {
        let bounds = self.slider_bounds.get();
        let width = f32::from(bounds.size.width).max(1.);
        let ratio = (f32::from(x - bounds.origin.x) / width).clamp(0., 1.);
        let range = if self.draft.high_fps { 336. } else { 36. };
        self.draft.fps = (24. + ratio * range).round() as u32;
    }
    fn combo(&self, key: &'static str, selected: u32, options: Vec<(u32, SharedString)>, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let label = options.iter().find(|(id, _)| *id == selected).map(|(_, text)| text.clone())
            .unwrap_or_else(|| selected.to_string().into());
        let mut field = div().relative().w(px(270.)).max_w(relative(1.)).flex_none()
            .child(div().id(key).flex().items_center().justify_between().px(px(10.)).h(px(32.))
                .bg(t.raised).border_1().border_color(t.border).cursor_pointer()
                .child(div().flex_1().min_w_0().truncate().child(label)).child("▾")
                .on_click(cx.listener(move |v, _, _, cx| { v.menu = if v.menu == Some(key) { None } else { Some(key) }; cx.notify(); })));
        if self.menu == Some(key) {
            let mut menu = div().id(SharedString::from(format!("menu-{key}"))).absolute().left_0()
                .when(key == "language", |s| s.bottom(px(34.)))
                .when(key != "language", |s| s.top(px(34.)))
                .w(px(270.)).max_w(relative(1.)).max_h(px(240.)).overflow_y_scroll().bg(t.raised).border_1().border_color(t.accent).occlude();
            for (value, text) in options {
                let device_id = match key {
                    "audio_sink" => value.checked_sub(1).and_then(|i|self.device_inventory.sinks.get(i as usize)).map(|d|d.id.clone()),
                    "audio_source" => value.checked_sub(1).and_then(|i|self.device_inventory.sources.get(i as usize)).map(|d|d.id.clone()),
                    "camera" => value.checked_sub(2).and_then(|i|self.device_inventory.cameras.get(i as usize)).map(|d|d.id.clone()),
                    "gpu" => value.checked_sub(1).and_then(|i|self.gpus.get(i as usize)).map(|d|d.id.clone()),
                    _=>None,
                };
                menu = menu.child(div().id((key, value as usize)).px(px(10.)).py(px(7.)).whitespace_normal().cursor_pointer()
                    .text_color(if value == selected { t.accent } else { t.text })
                    .hover(|s| s.bg(t.surface)).child(text)
                    .on_click(cx.listener(move |v, _, _, cx| {
                        match key {
                            "phone_profile"=>v.phone_id=if value==0 {"auto".into()} else {liw_core::phone::PROFILES[(value-1) as usize].id.into()},
                            "audio_sink" => v.devices.audio_sink = device_id.clone(),
                            "audio_source" => v.devices.audio_source = device_id.clone(),
                            "camera" => v.devices.camera = match value { 0=>"auto".into(),1=>"off".into(),_=>device_id.clone().unwrap_or_else(||"auto".into()) },
                            "gpu" => v.graphics.gpu = device_id.clone(),
                            "orientation" => { if let Some(d) = &mut v.display { d.orient(value == 1); } }
                            "resolution" => {
                                v.custom_resolution = value == 6;
                                if value < 6 { if let Some(d) = &mut v.display { d.preset(value as usize); } }
                            }
                            "dpi" => { if let Some(d) = &mut v.display { d.dpi = value; } }
                            "cpu" => v.draft.cpus = value,
                            "ram" => v.draft.memory_mib = value,
                            "mode" if value < 3 => {
                                let (cpus, memory, fps) = match value { 0 => (2, 2048, 60), 1 => (4, 4096, 60), _ => (0, 0, 240) };
                                v.draft.cpus = cpus.min(cpu_count());
                                v.draft.memory_mib = memory.min(memory_limit());
                                v.draft.fps = fps;
                                v.draft.high_fps = fps > 60;
                            }
                            "language" => v.draft.language = v.languages[value as usize].id.clone(),
                            _ => {}
                        }
                        v.menu = None;
                        v.editing = None;
                        cx.notify();
                    })));
            }
            field = field.child(deferred(menu).with_priority(2));
        }
        field.into_any_element()
    }
    fn text(&self, key: &str) -> SharedString {
        let text = self.languages.iter().find(|l| l.id == self.draft.language)
            .and_then(|l| l.strings.get(key))
            .or_else(|| self.languages.iter().find(|l| l.id == "en").and_then(|l| l.strings.get(key)));
        SharedString::from(text.cloned().unwrap_or_else(|| key.into()))
    }
    fn import(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        cx.spawn(async move |view, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                if let Some(path) = paths.first() {
                    let result = read_language(path).and_then(|language| {
                        let dir = directory().join("languages");
                        std::fs::create_dir_all(&dir)?;
                        atomic_json(&dir.join(format!("{}.json", language.id)), &language)?;
                        Ok(language.id)
                    });
                    let _ = view.update(cx, |v, cx| {
                        match result {
                            Ok(id) => { v.languages = languages(); v.draft.language = id; v.message.clear(); }
                            Err(e) => v.message = e.to_string(),
                        }
                        cx.notify();
                    });
                }
            }
        }).detach();
    }
    fn choose_screenshots(&mut self,cx: &mut Context<Self>) {
        let paths=cx.prompt_for_paths(PathPromptOptions {files:false,directories:true,multiple:false,prompt:None});
        cx.spawn(async move |view,cx| {
            if let Ok(Ok(Some(paths)))=paths.await {
                if let Some(path)=paths.first() {
                    let _=view.update(cx,|v,cx| { v.draft.screenshots=Some(path.clone()); cx.notify(); });
                }
            }
        }).detach();
    }
    fn shortcuts_content(&self,t:&Theme,cx:&mut Context<Self>)->AnyElement {
        use crate::shortcuts::{Action,ACTIONS};
        let mut rows=div().flex().flex_col().gap(px(8.));
        for action in ACTIONS {
            let binding=&self.shortcuts.bindings[&action];
            let label=if self.recording==Some(action) {self.text("shortcut_recording")}
                else if let Some(chord)=binding.chord {chord.label().into()}
                else {self.text(if action==Action::CaptureMouse {"shortcut_from_profile"} else {"shortcut_unassigned"})};
            rows=rows.child(div().flex().flex_wrap().items_center().gap(px(12.)).py(px(10.)).border_b_1().border_color(t.border)
                .child(div().flex_1().min_w(px(130.)).whitespace_normal().child(self.text(action.text())))
                .child(button(label,SharedString::from(format!("record-{action:?}")),self.recording==Some(action),t)
                    .w(px(190.)).on_click(cx.listener(move |v,_,window,cx| {v.recording=Some(action);v.pending_physical=v.physical_shortcuts.then_some(action);v.message.clear();window.focus(&v.focus,cx);cx.notify();})))
                .child(div().id(SharedString::from(format!("enable-{action:?}"))).cursor_pointer().p(px(8.)).child(toggle(binding.enabled,t))
                    .on_click(cx.listener(move |v,_,_,cx| {
                        let b=v.shortcuts.bindings.get_mut(&action).unwrap();b.enabled=!b.enabled;v.recording=None;v.pending_physical=None;cx.notify();
                    }))));
        }
        let conflict=self.shortcuts.conflicts_with_capture(self.profile_capture).map(|(a,b)|format!("{}: {} / {}",self.text("shortcut_conflict"),self.text(a.text()),self.text(b.text())));
        div().id("shortcuts-scroll").flex_1().min_h_0().min_w_0().overflow_y_scroll().px(px(30.)).py(px(22.))
            .child(div().w_full().max_w(px(620.)).flex().flex_col().gap(px(14.))
                .child(div().whitespace_normal().text_color(t.text_muted).child(self.text("shortcut_help")))
                .child(rows)
                .when_some(conflict,|s,text|s.child(div().whitespace_normal().text_color(t.text).child(SharedString::from(text))))
                .when(!self.shortcuts_error.is_empty(),|s|s.child(div().whitespace_normal().child(SharedString::from(self.shortcuts_error.clone()))))
                .child(div().whitespace_normal().text_size(px(12.)).text_color(t.text_muted).child(self.text("shortcut_linux_help")))
                .child(div().flex().flex_wrap().gap(px(10.))
                    .child(button(self.text("shortcut_defaults"),"shortcut-defaults",false,t).on_click(cx.listener(|v,_,_,cx| {
                        v.shortcuts=Default::default();v.recording=None;v.pending_physical=None;v.shortcuts_error.clear();v.message=v.text("shortcut_defaults_pending").to_string();cx.notify();
                    })))
                    .child(button(self.text("shortcut_profile_reset"),"shortcut-profile-reset",false,t).on_click(cx.listener(|v,_,_,cx| {
                        let b=v.shortcuts.bindings.get_mut(&Action::CaptureMouse).unwrap();b.enabled=true;b.chord=None;v.recording=None;v.pending_physical=None;cx.notify();
                    })))
                    .child(button(self.text("shortcut_reload"),"shortcut-reload",false,t).on_click(cx.listener(|v,_,_,cx| {
                        match crate::shortcuts::Shortcuts::load() {Ok(config)=>{v.shortcuts=config;v.shortcuts_error.clear();v.recording=None;v.pending_physical=None;v.message.clear();},Err(e)=>v.shortcuts_error=e.to_string()}
                        cx.notify();
                    }))))).into_any_element()
    }
    fn shortcut_key(&mut self,e:&KeyDownEvent,cx:&mut Context<Self>) {
        let Some(action)=self.recording else {return;};
        if e.keystroke.key=="escape" {self.recording=None;self.pending_physical=None;cx.stop_propagation();cx.notify();return;}
        if !self.physical_shortcuts {if let Some(chord)=crate::shortcuts::Chord::from_event(e) {
            let b=self.shortcuts.bindings.get_mut(&action).unwrap();b.chord=Some(chord);b.enabled=true;
            self.recording=None;self.message.clear();
        }}
        cx.stop_propagation();cx.notify();
    }
    fn preferences_content(&self,t: &Theme,cx: &mut Context<Self>) -> AnyElement {
        let langs=self.languages.iter().enumerate().map(|(n,l)|(n as u32,l.name.clone().into())).collect();
        let language=self.languages.iter().position(|l|l.id==self.draft.language).unwrap_or(0) as u32;
        let language_combo=self.combo("language",language,langs,t,cx);
        let row=|key: &'static str,on: bool| div().id(key).flex().justify_between().items_center().gap(px(20.)).py(px(10.)).cursor_pointer()
            .child(div().flex_1().whitespace_normal().child(self.text(key))).child(toggle(on,t))
            .on_click(cx.listener(move |v,_,_,cx| {
                let value=match key {"fullscreen_start"=>&mut v.draft.fullscreen_start,"escape_fullscreen"=>&mut v.draft.escape_fullscreen,_=>&mut v.draft.escape_mouse};
                *value=!*value; cx.notify();
            }));
        let hint=|key|div().whitespace_normal().text_size(px(12.)).text_color(t.text_muted).child(self.text(key));
        div().id("preferences-scroll").flex_1().min_h_0().min_w_0().overflow_y_scroll().px(px(30.)).py(px(22.))
            .child(div().w_full().max_w(px(580.)).flex().flex_col().gap(px(12.))
                .child(self.text("language")).child(language_combo)
                .child(button(self.text("import"),"import-language",false,t).on_click(cx.listener(|v,_,_,cx|v.import(cx))))
                .child(hint("language_help"))
                .child(div().h(px(8.)).border_b_1().border_color(t.border))
                .child(self.text("window_preferences"))
                .child(row("fullscreen_start",self.draft.fullscreen_start))
                .child(hint("fullscreen_start_help"))
                .child(div().h(px(8.)).border_b_1().border_color(t.border))
                .child(self.text("escape_behavior"))
                .child(row("escape_fullscreen",self.draft.escape_fullscreen))
                .child(row("escape_mouse",self.draft.escape_mouse))
                .child(hint("escape_help"))
                .child(div().h(px(8.)).border_b_1().border_color(t.border))
                .child(self.text("screenshots"))
                .child(div().whitespace_normal().text_color(t.text_muted).child(SharedString::from(self.draft.screenshot_directory().display().to_string())))
                .child(div().flex().flex_wrap().gap(px(8.))
                    .child(button(self.text("choose_folder"),"screenshots-folder",false,t).on_click(cx.listener(|v,_,_,cx|v.choose_screenshots(cx))))
                    .child(button(self.text("system_default"),"screenshots-reset",false,t).on_click(cx.listener(|v,_,_,cx| {v.draft.screenshots=None;cx.notify();}))))
                .child(hint("screenshots_help")))
            .into_any_element()
    }
    fn save(&mut self, cx: &mut Context<Self>) {
        if self.busy { return; }
        if self.tab=="advanced" {
            if self.advanced.is_none() || self.probing_advanced {self.message=self.text("advanced_unavailable").to_string();cx.notify();return;}
            self.busy=true;self.message=self.text("applying").to_string();let settings=self.advanced_draft;let root=self.root_draft;
            cx.spawn(async move |view,cx| {
                let result=gpui_tokio::Tokio::spawn(cx,crate::advanced::save(settings,root)).await;
                let _=view.update(cx,|v,cx| {
                    v.busy=false;
                    match result {Ok(Ok(state))=>{v.advanced_draft=state.settings;v.root_draft=state.root.requested;v.message=v.text(if state.root.pending {"root_saved"} else {"advanced_saved"}).to_string();v.advanced=Some(state);},Ok(Err(e))=>{v.advanced=None;v.message=e;},Err(e)=>{v.advanced=None;v.message=e.to_string();}}
                    cx.notify();
                });
            }).detach();return;
        }
        if self.tab=="shortcuts" {
            if self.recording.is_some() || self.pending_physical.is_some() {self.message=self.text("shortcut_recording").to_string();}
            else if !self.shortcuts_error.is_empty() {self.message=self.text("shortcut_bad_file").to_string();}
            else if let Some((a,b))=self.shortcuts.conflicts_with_capture(self.profile_capture) {self.message=format!("{}: {} / {}",self.text("shortcut_conflict"),self.text(a.text()),self.text(b.text()));}
            else {self.message=match self.shortcuts.save() {Ok(())=>self.text("shortcut_saved").to_string(),Err(e)=>e.to_string()};}
            cx.notify();return;
        }
        if self.tab=="phone" {
            if !self.phone_loaded || self.probing_phone {self.message=self.text("phone_unavailable").to_string();cx.notify();return;}
            self.busy=true;self.message=self.text("applying").to_string();
            let id=self.phone_id.clone();
            cx.spawn(async move |view,cx| {
                let result=gpui_tokio::Tokio::spawn(cx,async move {
                    let helper=liw_core::HelperClient::connect().await.map_err(|e|e.to_string())?;
                    helper.set_phone_profile(&id).await.map_err(|e|e.to_string())
                }).await;
                let _=view.update(cx,|v,cx| {
                    v.busy=false;
                    v.message=match result {Ok(Ok(()))=>v.text("phone_saved").to_string(),Ok(Err(e))=>e,Err(e)=>e.to_string()};
                    cx.notify();
                });
            }).detach();
            return;
        }
        if self.tab=="preferences" {
            self.message=match self.draft.save_general_in(&directory()) {
                Ok(())=>self.text("preferences_saved").to_string(),Err(e)=>e.to_string(),
            };
            cx.notify(); return;
        }
        if self.tab == "devices" {
            let saved = self.devices.validate().map_err(anyhow::Error::msg).and_then(|()| {
                std::fs::create_dir_all(directory())?;
                atomic_json(&liw_core::devices::DeviceSettings::path(),&self.devices)
            });
            if let Err(e) = saved { self.message=e.to_string(); cx.notify(); return; }
            self.busy=true;
            self.message=self.text("applying").to_string();
            let settings=self.devices.clone();
            cx.spawn(async move |view,cx| {
                let result=gpui_tokio::Tokio::spawn(cx,async move {
                    let mut errors=Vec::new();
                    match liw_core::helper::HelperClient::connect().await {
                        Ok(helper)=>if let Err(e)=helper.set_camera(&settings.camera).await { errors.push(e.to_string()); },
                        Err(e)=>errors.push(e.to_string()),
                    }
                    if let Err(e)=liw_core::devices::route(&settings).await { errors.push(e); }
                    errors
                }).await;
                let _=view.update(cx,|v,cx| {
                    v.busy=false;
                    v.message=match result {
                        Ok(errors) if errors.is_empty()=>v.text("devices_saved").to_string(),
                        Ok(errors)=>format!("{} {}",v.text("devices_apply_error"),errors.join(" · ")),
                        Err(e)=>format!("{} {e}",v.text("devices_apply_error")),
                    };
                    cx.notify();
                });
            }).detach();
            return;
        }
        if self.tab == "graphics" {
            let result = (|| -> anyhow::Result<()> {
                let cfg = std::fs::read_to_string("/var/lib/waydroid/waydroid.cfg").unwrap_or_default();
                self.graphics.select(&liw_core::graphics::discover(),liw_core::nvidia::source_native_selected(&cfg)).map_err(anyhow::Error::msg)?;
                std::fs::create_dir_all(directory())?;
                atomic_json(&liw_core::graphics::GraphicsSettings::path(),&self.graphics)
            })();
            self.message = match result { Ok(()) => self.text("graphics_saved").to_string(), Err(e) => e.to_string() };
            cx.notify();
            return;
        }
        if self.tab == "display" {
            self.message = match self.display.as_ref().ok_or_else(|| anyhow::anyhow!("Invalid display profile")).and_then(|d| d.save()) {
                Ok(()) => self.text("display_saved").to_string(),
                Err(e) => e.to_string(),
            };
            cx.notify();
            return;
        }
        if let Err(e) = self.draft.save() { self.message = e.to_string(); cx.notify(); return; }
        self.busy = true;
        self.message = self.text("applying").to_string();
        let p = self.draft.clone();
        cx.spawn(async move |view, cx| {
            let result = gpui_tokio::Tokio::spawn(cx, async move {
                let helper = liw_core::helper::HelperClient::connect().await.map_err(|e| e.to_string())?;
                helper.set_refresh_rate(p.fps).await.map_err(|e| e.to_string())?;
                helper.set_resources(p.cpus, p.memory_mib).await.map_err(|e| e.to_string())
            }).await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = view.update(cx, |v, cx| {
                v.busy = false;
                v.message = match result {
                    Ok(()) => v.text("saved").to_string(),
                    Err(e) => format!("{} {e}", v.text("apply_failed")),
                };
                cx.notify();
            });
        }).detach();
    }
}
fn button(label: impl Into<SharedString>, id: impl Into<ElementId>, active: bool, t: &Theme) -> Stateful<Div> {
    div().id(id).px(px(12.)).py(px(8.)).rounded(px(6.)).border_1()
        .border_color(if active { t.accent } else { t.border }).bg(t.raised)
        .cursor_pointer().hover(|s| s.bg(t.surface)).child(label.into())
}
impl Render for Performance {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::dark();
        if !self.embedded { window.set_window_title(&self.text("settings")); }
        let cpu = (0..=cpu_count()).map(|n| (n, if n == 0 { self.text("auto") } else { format!("{n}").into() })).collect();
        let mut memory_values = vec![0, 1024, 2048, 4096, 6144, 8192, 12288, 16384];
        memory_values.push(self.draft.memory_mib);
        memory_values.sort_unstable(); memory_values.dedup();
        let ram = memory_values.into_iter().filter(|n| *n <= memory_limit())
            .map(|n| (n, if n == 0 { self.text("auto") } else { format!("{} MiB", n).into() })).collect();
        let mode = match (self.draft.cpus, self.draft.memory_mib, self.draft.fps) {
            (2, 2048, 60) => 0, (4, 4096, 60) => 1, (0, 0, 240) => 2, _ => 3,
        };
        let modes = ["low", "balanced", "high", "custom"].into_iter().enumerate().map(|(n,k)| (n as u32, self.text(k))).collect();
        let nav_width=if self.embedded && f32::from(window.viewport_size().width)<650. {145.} else {205.};
        let mut nav = div().w(px(nav_width)).flex_none().h_full().p(px(22.)).bg(t.surface).flex().flex_col().gap(px(12.));
        nav = nav.child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).mb(px(18.)).child(self.text("settings")));
        for key in ["performance", "display", "graphics", "devices", "preferences", "phone", "shortcuts", "advanced", "about"] {
            nav = nav.child(div().id(key).whitespace_normal().text_size(px(13.)).py(px(5.)).border_l_2().pl(px(10.))
                .border_color(if key == self.tab { t.accent } else { t.surface })
                .text_color(if key == self.tab { t.text } else if matches!(key, "performance" | "display" | "graphics" | "devices" | "preferences" | "phone" | "shortcuts" | "advanced" | "about") { t.text_muted } else { t.text_faint })
                .child(self.text(key)).when(matches!(key, "performance" | "display" | "graphics" | "devices" | "preferences" | "phone" | "shortcuts" | "advanced" | "about"), |s| s.cursor_pointer()
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.tab = key; v.menu = None; v.editing = None; v.recording=None; v.pending_physical=None; v.message.clear();
                        if key == "graphics" { v.refresh_graphics(cx); }
                        if key == "devices" { v.refresh_devices(cx); }
                        if key == "about" { v.refresh_about(cx); }
                        if key == "phone" { v.refresh_phone(cx); }
                        if key == "advanced" { v.refresh_advanced(cx); }
                        cx.notify();
                    }))));
        }
        let bounds = self.slider_bounds.clone();
        let max_fps = if self.draft.high_fps { 360 } else { 60 };
        let ratio = (self.draft.fps.clamp(24,max_fps) - 24) as f32 / (max_fps - 24) as f32;
        let accent = t.accent;
        let line = t.border;
        let slider = div().id("fps-slider").w_full().h(px(28.)).cursor_pointer()
            .child(canvas(move |b, _, _| bounds.set(b), move |b, (), window, _| {
                let start = b.origin + point(px(0.), px(14.));
                let end = start + point(b.size.width, px(0.));
                let knob = start + point(b.size.width * ratio, px(0.));
                for (to, color) in [(end, line), (knob, accent)] {
                    let mut p = PathBuilder::stroke(px(3.)); p.move_to(start); p.line_to(to);
                    if let Ok(p) = p.build() { window.paint_path(p, color); }
                }
                let mut p = PathBuilder::stroke(px(7.));
                for i in 0..=24 {
                    let a = i as f32 * std::f32::consts::TAU / 24.;
                    let q = knob + point(px(a.cos() * 3.), px(a.sin() * 3.));
                    if i == 0 { p.move_to(q); } else { p.line_to(q); }
                }
                if let Ok(p) = p.build() { window.paint_path(p, accent); }
            }).size_full())
            .on_mouse_down(MouseButton::Left, cx.listener(|v, e: &MouseDownEvent, _, cx| { v.dragging = true; v.slider_value(e.position.x); cx.notify(); }));
        let hint = |text: SharedString| div().w_full().min_w_0().whitespace_normal().text_size(px(12.)).text_color(t.text_muted).child(text);
        let content = div().id("performance-scroll").flex_1().min_h_0().min_w_0().overflow_y_scroll().px(px(30.)).py(px(22.))
            .child(div().w_full().max_w(px(580.)).flex().flex_col().gap(px(10.))
                .child(self.text("cpu")).child(self.combo("cpu", self.draft.cpus, cpu, &t, cx))
                .child(div().h(px(10.))).child(self.text("ram")).child(self.combo("ram", self.draft.memory_mib, ram, &t, cx))
                .child(hint(self.text("resource_help")))
                .child(div().h(px(10.))).child(self.text("mode")).child(self.combo("mode", mode, modes, &t, cx))
                .child(div().h(px(10.))).child(SharedString::from(format!("{} · {}", self.text("fps"), self.draft.fps)))
                .child(slider)
                .child(div().flex().justify_between().text_size(px(11.)).text_color(t.text_muted).child("24").child(SharedString::from(max_fps.to_string())))
                .child(hint(self.text("fps_help")))
                .child(div().id("high-fps").flex().justify_between().items_center().py(px(8.)).cursor_pointer()
                    .child(self.text("high_fps")).child(toggle(self.draft.high_fps, &t))
                    .on_click(cx.listener(|v, _, _, cx| {
                        v.draft.high_fps = !v.draft.high_fps;
                        if !v.draft.high_fps { v.draft.fps = v.draft.fps.min(60); }
                        cx.notify();
                    })))
                .child(div().id("show-fps").flex().justify_between().items_center().py(px(8.)).cursor_pointer()
                    .child(self.text("show_fps")).child(toggle(self.draft.show_fps, &t))
                    .on_click(cx.listener(|v, _, _, cx| { v.draft.show_fps = !v.draft.show_fps; cx.notify(); })))
                .child(hint(self.text("vsync"))));
        let content = match self.tab { "advanced"=>self.advanced_content(&t,cx), "shortcuts"=>self.shortcuts_content(&t,cx), "phone"=>self.phone_content(&t,cx), "preferences"=>self.preferences_content(&t,cx), "display" => self.display_content(&t,cx), "graphics" => self.graphics_content(&t,cx), "devices" => self.devices_content(&t,cx), "about" => self.about_content(&t,cx), _ => content.into_any_element() };
        div().track_focus(&self.focus).on_key_down(cx.listener(|v, e: &KeyDownEvent, window, cx| {
            if v.recording.is_some() {v.shortcut_key(e,cx);}
            else if e.keystroke.key=="escape" && v.editing.is_none() { v.close(window,cx); }
            else { v.numeric_key(e,cx); }
        }))
            .size_full().flex().bg(t.bg).text_color(t.text).text_size(px(13.)).font_family("sans-serif")
            .on_mouse_move(cx.listener(|v, e: &MouseMoveEvent, _, cx| {
                if v.dragging && e.pressed_button == Some(MouseButton::Left) { v.slider_value(e.position.x); cx.notify(); }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|v, _, _, _| v.dragging = false))
            .child(nav)
            .child(div().flex_1().min_w_0().h_full().flex().flex_col()
                .child(content)
                .child(div().flex_none().px(px(30.)).py(px(16.)).border_t_1().border_color(t.border).flex().flex_col().gap(px(10.))
                    .child(hint(SharedString::from(self.message.clone())))
                    .child(div().flex().flex_wrap().justify_end().gap(px(10.))
                        .child(button(self.text("close"), "close-performance", false, &t).on_click(cx.listener(|v,_,window,cx|v.close(window,cx))))
                        .when(self.tab != "about",|s|s.child(button(self.text(if self.busy { "applying" } else if self.tab != "performance" { "save_display" } else { "save" }), "save-performance", true, &t)
                            .on_click(cx.listener(|v, _, _, cx| {
                                if v.editing.is_some() { v.message = v.text("finish_number").to_string(); cx.notify(); }
                                else { v.save(cx); }
                            })))))))
    }
}
pub struct SettingsClosed;
impl EventEmitter<SettingsClosed> for Performance {}

fn toggle(on: bool, t: &Theme) -> impl IntoElement {
    div().w(px(32.)).h(px(18.)).rounded(px(9.)).px(px(3.)).flex().items_center()
        .bg(if on { t.accent } else { t.border })
        .when(on, |s| s.justify_end())
        .child(div().size(px(12.)).rounded(px(6.)).bg(t.text))
}
pub fn open(cx: &mut App) -> anyhow::Result<()> {
    if let Some(window) = cx.windows().iter().find_map(|w| w.downcast::<Performance>()) {
        window.update(cx, |_, window, _| window.activate_window())?;
        return Ok(());
    }
    cx.open_window(WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(900.), px(720.)), cx))),
        window_min_size: Some(size(px(760.), px(560.))),
        app_id: Some("id.nexdroid.settings".into()), ..Default::default()
    }, |_, cx| cx.new(Performance::new))?;
    Ok(())
}

/// A toothed gear outline, drawn independently of the system symbol font.
pub fn gear(color: Hsla) -> impl IntoElement {
    canvas(|_, _, _| (), move |bounds, (), window, _| {
        let mut path = PathBuilder::stroke(px(1.7));
        for i in 0..=64 {
            let angle = i as f32 * std::f32::consts::TAU / 64.;
            let radius = if i % 8 < 4 { 12. } else { 9. };
            let point = bounds.origin + point(px(16. + radius * angle.cos()), px(16. + radius * angle.sin()));
            if i == 0 { path.move_to(point); } else { path.line_to(point); }
        }
        for i in 0..=32 {
            let angle = i as f32 * std::f32::consts::TAU / 32.;
            let point = bounds.origin + point(px(16. + 4. * angle.cos()), px(16. + 4. * angle.sin()));
            if i == 0 { path.move_to(point); } else { path.line_to(point); }
        }
        if let Ok(path) = path.build() { window.paint_path(path, color); }
    }).size(px(32.))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[core::prelude::v1::test]
    fn general_preferences_save_preserves_resources_display_and_unknown_fields() {
        let dir=tempfile::tempdir().unwrap();
        let path=dir.path().join("performance.json");
        std::fs::write(&path,r#"{"cpus":2,"memory_mib":4096,"fps":360,"future":"keep"}"#).unwrap();
        std::fs::write(dir.path().join("display.json"),"unchanged").unwrap();
        let draft=Preferences { language:"en".into(),escape_fullscreen:false,screenshots:Some(dir.path().join("shots")),..Default::default() };
        draft.save_general_in(dir.path()).unwrap();
        let value:serde_json::Value=serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(value["cpus"],2); assert_eq!(value["memory_mib"],4096); assert_eq!(value["fps"],360);
        assert_eq!(value["future"],"keep"); assert_eq!(value["language"],"en"); assert_eq!(value["escape_fullscreen"],false);
        assert_eq!(std::fs::read_to_string(dir.path().join("display.json")).unwrap(),"unchanged");
    }
    #[core::prelude::v1::test]
    fn older_preferences_keep_escape_and_window_defaults() {
        let old:Preferences=serde_json::from_str(r#"{"fps":360,"language":"en"}"#).unwrap();
        assert!(old.escape_fullscreen && old.escape_mouse); assert!(!old.fullscreen_start);
        assert_eq!(old.fps,360); assert!(old.screenshots.is_none());
    }
    #[core::prelude::v1::test]
    fn invalid_screenshot_path_does_not_overwrite_preferences() {
        let dir=tempfile::tempdir().unwrap();
        let path=dir.path().join("performance.json");
        std::fs::write(&path,"{}").unwrap();
        let draft=Preferences { screenshots:Some("relative".into()),..Default::default() };
        assert!(draft.save_general_in(dir.path()).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(),"{}");
    }
    #[core::prelude::v1::test]
    fn saving_performance_retains_existing_display_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("display.json");
        std::fs::write(&path, r#"{"width":1920,"height":1080,"dpi":400,"fps":240,"future":"keep"}"#).unwrap();
        Preferences { fps: 144, ..Default::default() }.save_in(dir.path()).unwrap();
        let display: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(display["width"], 1920);
        assert_eq!(display["dpi"], 400);
        assert_eq!(display["future"], "keep");
        assert_eq!(display["fps"], 144);
        assert!(dir.path().join("performance.json").exists());
    }
    #[core::prelude::v1::test]
    fn built_in_languages_cover_the_same_keys_and_partial_import_falls_back() {
        let ru: Language = serde_json::from_str(include_str!("../locales/ru.json")).unwrap();
        let en: Language = serde_json::from_str(include_str!("../locales/en.json")).unwrap();
        assert_eq!(ru.strings.keys().collect::<Vec<_>>(), en.strings.keys().collect::<Vec<_>>());
        let custom = Language { id: "custom".into(), name: "Custom".into(), strings: BTreeMap::new() };
        assert_eq!(custom.strings.get("performance").or_else(|| en.strings.get("performance")).unwrap(), "Performance");
    }
}
