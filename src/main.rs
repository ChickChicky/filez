use crosscurses::*;
use std::{
    env::{
        args as cmdargs,
        consts,
        current_dir,
    },
    fs:: {
        self,
        DirEntry, Metadata,
    },
    path::*,
    process::Command,
    sync::{
        Arc,
        Mutex,
    },
    thread,
    vec, 
    time::{ Duration, UNIX_EPOCH }, 
    collections::HashMap,
};
use chrono::{
    DateTime, Local,
};
use iota::iota;

struct Icon<'a> {
    m : fn(&str,FileStat) -> bool,
    icon : &'a str,
    color : i16,
}

iota! {
    const ICON_COLOR_PAIR_NONE: i16 = iota;
    , FILE_COLOR_PAIR_DIR
    , FILE_COLOR_PAIR_FILE
    , FILE_COLOR_PAIR_EXTRA

    , ICON_COLOR_PAIR_RUST
    , ICON_COLOR_PAIR_GIT
    , ICON_COLOR_PAIR_CONFIG
    , ICON_COLOR_PAIR_LOCK
    , ICON_COLOR_PAIR_JSON
    , ICON_COLOR_PAIR_JS
    , ICON_COLOR_PAIR_SVG
    , ICON_COLOR_PAIR_HTML
    , ICON_COLOR_PAIR_CSS
    , ICON_COLOR_PAIR_FONT
}

const ICONS: &[Icon] = &[
    Icon {
        m: |_f,_p| _f.ends_with(".rs"),
        icon: "\u{e7a8}",
        color: ICON_COLOR_PAIR_RUST,
    },
    Icon{
        m: |_f,_p| (_f == ".git" && _p.is_dir()) || (_f == ".gitignore" && _p.is_file()) || (_p.is_file() && (_f == "HEAD" || _f == "FETCH_HEAD" || _f == "description" || _f == "config") && _p.parent().file_name == ".git"),
        icon: "\u{e702}",
        color: ICON_COLOR_PAIR_GIT,
    },
    Icon {
        m: |_f,_p| _f.ends_with(".toml") || _f.ends_with(".toml"),
        icon: "\u{f013}",
        color: ICON_COLOR_PAIR_CONFIG,
    },
    Icon {
        m: |_f,_p| _f.ends_with(".lock"),
        icon: "\u{f023}",
        color: ICON_COLOR_PAIR_LOCK,
    },
    Icon {
        m: |_f,_p| _f.ends_with(".js") || _f == "package.json" || _f == "node_modules",
        icon: "\u{e718}",
        color: ICON_COLOR_PAIR_JS,
    },
    Icon {
        m: |_f,_p| _f.ends_with(".json") || _f.ends_with(".jsonc") || _f.ends_with(".jsonl"),
        icon: "\u{e60b}",
        color: ICON_COLOR_PAIR_JSON,
    },
    Icon {
        m: |_f,_p| _f.ends_with(".svg") || _f.ends_with(".png") || _f.ends_with(".jpg") || _f.ends_with(".jpeg"),
        icon: "\u{e701}",
        color: ICON_COLOR_PAIR_SVG,
    },
    Icon {
        m: |_f,_p| _f.ends_with(".css"),
        icon: "\u{f13c}",
        color: ICON_COLOR_PAIR_CSS,
    },
    Icon {
        m: |_f,_p| _f.ends_with(".html"),
        icon: "\u{f13b}",
        color: ICON_COLOR_PAIR_HTML,
    },
    Icon {
        m: |_f,_p| _f.ends_with(".woff2") || _f.ends_with(".ttf"),
        icon: "\u{f031}",
        color: ICON_COLOR_PAIR_FONT,
    },
    Icon {
        m: |_f,_p| _p.is_dir(),
        icon: "\u{f07b}",
        color: ICON_COLOR_PAIR_NONE,
    },
    Icon {
        m: |_f,_p| _f.ends_with(".txt"),
        icon: "\u{f15c}",
        color: ICON_COLOR_PAIR_NONE,
    },
    Icon {
        m: |_f,_p| _p.is_file(),
        icon: "\u{f15b}",
        color: ICON_COLOR_PAIR_NONE,
    },
];

#[derive(Clone)]
/// Stores information about a file and provides some small helpers
struct FileStat {
    typ: u32,
    path: String,
    file_name: String,

}
impl Into<FileStat> for DirEntry {
    fn into(self) -> FileStat {
        self.path().into()
    }
}
impl Into<FileStat> for PathBuf {
    fn into(self) -> FileStat {
        FileStat {
            typ: 0 | (if self.is_dir() {FileStat::TYPE_DIR} else {0}) | (if self.is_file() {FileStat::TYPE_FILE} else {0}),
            path: self.to_str().unwrap().to_string(),
            file_name: self.file_name().unwrap().to_str().unwrap().to_string()
        }   
    }
}
impl FileStat {
    const TYPE_FILE : u32 = 1;
    const TYPE_DIR  : u32 = 2;
    
    /// Returns whether the file a directory
    pub fn is_dir(&self) -> bool {
        (self.typ & FileStat::TYPE_DIR) != 0
    }
    /// Returns whether the file is a file
    pub fn is_file(&self) -> bool {
        (self.typ & FileStat::TYPE_FILE) != 0
    }

    /// Returns the path of the file 
    pub fn path(&self) -> &str {
        self.path.as_str()
    }
    /// Returns the lowest portion of the file path
    pub fn file_name(&self) -> &str {
        self.file_name.as_str()
    }
    /// Returns a new FileStat of the parent of the file
    pub fn parent(&self) -> FileStat {
        let temp: PathBuf = PathBuf::from(self.path.as_str());
        let parent: PathBuf = temp.parent().unwrap().to_path_buf();
        parent.into()
    }

    /// Returns the metadata of the file
    pub fn metadata(&self) -> Metadata {
        fs::metadata(self.path()).unwrap()
    }

}

#[derive(Clone)]
struct FileWatcher {
    path: Arc<Mutex<PathBuf>>,
    path2: Arc<Mutex<PathBuf>>,
    filez: Arc<Mutex<Vec<FileStat>>>,
}
impl FileWatcher {

    pub fn new(path: Option<String>) -> Self {
        FileWatcher {
            path: Arc::from(Mutex::from(path.map(PathBuf::from).unwrap_or_else(|| current_dir().unwrap()))),
            path2: Arc::from(Mutex::from(PathBuf::from(""))),
            filez: Arc::default()
        }
    }

    pub fn path(&self) -> PathBuf {
        self.path.lock().unwrap().clone()
    }
    pub fn set_path(&self, pathfn: Box<dyn Fn(&mut PathBuf)->()>) {
        pathfn(&mut *self.path.lock().unwrap());
    }

    pub fn filez(&self) -> Vec<FileStat> {
        self.filez.lock().unwrap().clone()
    }
    pub fn set_filez(&self, filezfn: Box<dyn Fn(&mut Vec<FileStat>)->()>) {
        filezfn(&mut *self.filez.lock().unwrap());
    }

    pub fn path2(&self) -> PathBuf {
        self.path2.lock().unwrap().clone()
    }
    pub fn set_path2(&self, pathfn: Box<dyn Fn(&mut PathBuf)->()>) {
        pathfn(&mut *self.path2.lock().unwrap());
    }

}

#[derive(Clone)]
#[derive(Copy)]
struct View {
    selected: i32,
    scroll: i32
}

fn main() {
    let mut args = cmdargs();

    let win: Window = initscr();

    win.keypad(true);
    win.nodelay(true);
    mousemask(ALL_MOUSE_EVENTS, std::ptr::null_mut());
    noecho();
    curs_set(0);

    start_color();

    init_pair(FILE_COLOR_PAIR_DIR, COLOR_BLUE, COLOR_BLACK);
    init_pair(FILE_COLOR_PAIR_FILE, COLOR_WHITE, COLOR_BLACK);
    init_pair(FILE_COLOR_PAIR_EXTRA, COLOR_YELLOW, COLOR_BLACK);

    init_pair(ICON_COLOR_PAIR_GIT, COLOR_YELLOW, COLOR_BLACK);
    init_pair(ICON_COLOR_PAIR_RUST, COLOR_YELLOW, COLOR_BLACK);
    init_pair(ICON_COLOR_PAIR_CONFIG, COLOR_CYAN, COLOR_BLACK);
    init_pair(ICON_COLOR_PAIR_LOCK, COLOR_YELLOW, COLOR_BLACK);
    init_pair(ICON_COLOR_PAIR_JSON, COLOR_YELLOW, COLOR_BLACK);
    init_pair(ICON_COLOR_PAIR_JS, COLOR_GREEN, COLOR_BLACK);
    init_pair(ICON_COLOR_PAIR_SVG, COLOR_RED, COLOR_BLACK);
    init_pair(ICON_COLOR_PAIR_CSS, COLOR_BLUE, COLOR_BLACK);
    init_pair(ICON_COLOR_PAIR_HTML, COLOR_YELLOW, COLOR_BLACK);
    init_pair(ICON_COLOR_PAIR_FONT, COLOR_RED, COLOR_BLACK);

    let file_watcher: FileWatcher = FileWatcher::new(args.nth(1));
    
    let mut selected: i32 = 0;
    let mut selected_hist: HashMap<String,View> = HashMap::new();
    let mut scroll: i32 = 0;

    let thread_file_watcher: FileWatcher = file_watcher.clone();
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(100));
            let p = thread_file_watcher.path();
            let mut filez: Vec<FileStat> = vec![];
            if let Ok(entries) = fs::read_dir(p.as_path()) {
                for entry in entries {
                    if let Ok(entry) = entry {
                        filez.push(entry.into());
                    }
                }
            }
            filez.sort_by(|a: &FileStat, b : &FileStat| b.is_dir().partial_cmp(&a.is_dir()).unwrap() );
            thread_file_watcher.set_filez(Box::new(move|nfilez: &mut Vec<FileStat>|{filez.clone_into(nfilez);}));
            thread_file_watcher.set_path2(Box::new(move|path2: &mut PathBuf|{*path2=p.clone()}))
        }
    });

    loop {

        let path: PathBuf = file_watcher.path();
        let filez: Vec<FileStat> = file_watcher.filez();

        win.clear();

        win.mvaddstr(0, 0, path.to_str().unwrap());

        for i in 0i32..win.get_max_y()-2 {
            if i+scroll < 0 {continue}
            if i+scroll >= filez.len() as i32 {break}
            let entry: &FileStat = &filez[(i+scroll) as usize];

            win.mv(i+1 as i32,0);

            win.printw(" ");

            let mut found: bool = false;
            let file_name =  entry.file_name();
            for icon in ICONS {
                if (icon.m)(file_name,entry.to_owned()) {
                    win.attron(COLOR_PAIR(icon.color as u64));
                    win.printw(icon.icon);
                    win.attroff(COLOR_PAIR(icon.color as u64));
                    found = true;
                    break;
                }
            }
            if !found { win.printw("?"); }
            win.printw(" ");
            
            let ft: u64 = {
                if entry.is_dir() {
                    FILE_COLOR_PAIR_DIR
                }
                else if entry.is_file() {
                    FILE_COLOR_PAIR_FILE
                }
                else {
                    FILE_COLOR_PAIR_EXTRA
                }
            } as u64;

            if i+scroll == selected { win.attron(A_REVERSE); }
            win.attron(COLOR_PAIR(ft));
            win.printw(format!("{}",entry.file_name()));
            win.attroff(COLOR_PAIR(ft));
            if i+scroll == selected { win.attroff(A_REVERSE); }

            win.mv(i+1 as i32,25);
            win.clrtoeol();

            let meta: Metadata = entry.metadata();
            //format("%d-%m-%Y %H:%M");
            win.printw(format!(" {}",DateTime::from_timestamp((meta.accessed().unwrap().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64)+(Local::now().offset().local_minus_utc() as i64), 0).unwrap().format("%d-%m-%Y %H:%M")));
        }

        if filez.len() > 0 { selected = selected.clamp(0, filez.len() as i32-1); }

        if selected > win.get_max_y()-3+scroll {
            while selected > win.get_max_y()-3+scroll {scroll += 1;}
        }
        if selected < scroll {
            while selected < scroll {scroll -= 1;}
        }

        selected_hist.insert(path.to_str().unwrap().to_string(), View{selected,scroll});
        
        win.refresh();

        match win.getch() {
            Some(Input::Character(c)) => {
                if c == 'q' {
                    break
                }
                if c == '\x08' {
                    let old_path: PathBuf  = file_watcher.path();
                    file_watcher.set_path(Box::new(|path: &mut PathBuf|{
                        path.pop();
                    }));
                    while file_watcher.path2().to_str() == old_path.to_str() { /*thread::sleep(Duration::from_millis(100))*/ }
                    let nview: View = selected_hist.get(&file_watcher.path().to_str().unwrap().to_string()).copied().unwrap_or_else(||{
                        let mut i: usize = 0;
                        for f in file_watcher.filez() {
                            if f.file_name() == old_path.file_name().unwrap() {
                                return View {
                                    selected: i as i32,
                                    scroll: i as i32
                                };
                            }
                            i += 1;
                        }
                        View { 
                            selected: 0,
                            scroll: 0,
                        }
                    });
                    selected = nview.selected;
                    scroll = nview.scroll;
                }
                if c == '\x0a' {
                    let f: FileStat = file_watcher.filez()[selected as usize].clone();
                    if f.is_dir() {
                        let old_path: PathBuf  = file_watcher.path();
                        file_watcher.set_path(Box::new(move |path: &mut PathBuf|{
                            path.push(f.file_name());
                        }));
                        while file_watcher.path2().to_str() == old_path.to_str() { }
                        let nview: View = selected_hist.get(&file_watcher.path().to_str().unwrap().to_string()).copied().unwrap_or_else(||{
                            let mut i: usize = 0;
                            for f in file_watcher.filez() {
                                if f.file_name() == old_path.file_name().unwrap() {
                                    return View {
                                        selected: i as i32,
                                        scroll: i as i32
                                    };
                                }
                                i += 1;
                            }
                            View { 
                                selected: 0,
                                scroll: 0,
                            }
                        });
                        selected = nview.selected;
                        scroll = nview.scroll;
                    }
                    else {
                        if consts::OS == "windows" {
                            Command::new("explorer").arg(f.path()).spawn().unwrap();
                        }
                    }
                }
            }
            Some(Input::KeyMouse) => {
                if let Ok(evt) = getmouse() {
                    /*
                    Left:
                        4    : click
                        2    : press
                        1    : release
                    Right:
                        4096 : click
                        2048 : press
                        1024 : release
                    Middle:
                        128  : click
                        64   : press
                        32   : release
                    Scroll:
                        65536 : up
                        2097152 : down
                    */
                    if evt.bstate & 65536 != 0 { scroll -= 1; }
                    if evt.bstate & 2097152 != 0 { scroll += 1; }
                }
            },
            Some(Input::KeyDown) => {selected += 1;},
            Some(Input::KeyUp)   => {selected -= 1;},
            _ => {}
        }

    }

    endwin();

}
