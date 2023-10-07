#![allow(unused)]

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
        mpsc::{ self, Sender, Receiver }
    },
    thread,
    vec, time::Duration, borrow::BorrowMut,
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
        m: |f,p| f.ends_with(".rs"),
        icon: "\u{e7a8}",
        color: ICON_COLOR_PAIR_RUST,
    },
    Icon{
        m: |f,p| (f == ".git" && p.is_dir()) || (f == ".gitignore" && p.is_file()) || (p.is_file() && (f == "HEAD" || f == "FETCH_HEAD" || f == "description" || f == "config") && p.parent().file_name == ".git"),
        icon: "\u{e702}",
        color: ICON_COLOR_PAIR_GIT,
    },
    Icon {
        m: |f,p| f.ends_with(".toml") || f.ends_with(".toml"),
        icon: "\u{f013}",
        color: ICON_COLOR_PAIR_CONFIG,
    },
    Icon {
        m: |f,p| f.ends_with(".lock"),
        icon: "\u{f023}",
        color: ICON_COLOR_PAIR_LOCK,
    },
    Icon {
        m: |f,p| f.ends_with(".js") || f == "package.json" || f == "node_modules",
        icon: "\u{e718}",
        color: ICON_COLOR_PAIR_JS,
    },
    Icon {
        m: |f,p| f.ends_with(".json") || f.ends_with(".jsonc") || f.ends_with(".jsonl"),
        icon: "\u{e60b}",
        color: ICON_COLOR_PAIR_JSON,
    },
    Icon {
        m: |f,p| f.ends_with(".svg") || f.ends_with(".png") || f.ends_with(".jpg") || f.ends_with(".jpeg"),
        icon: "\u{e701}",
        color: ICON_COLOR_PAIR_SVG,
    },
    Icon {
        m: |f,p| f.ends_with(".css"),
        icon: "\u{f13c}",
        color: ICON_COLOR_PAIR_CSS,
    },
    Icon {
        m: |f,p| f.ends_with(".html"),
        icon: "\u{f13b}",
        color: ICON_COLOR_PAIR_HTML,
    },
    Icon {
        m: |f,p| f.ends_with(".woff2") || f.ends_with(".ttf"),
        icon: "\u{f031}",
        color: ICON_COLOR_PAIR_FONT,
    },
    Icon {
        m: |f,p| p.is_dir(),
        icon: "\u{f07b}",
        color: ICON_COLOR_PAIR_NONE,
    },
    Icon {
        m: |f,p| f.ends_with(".txt"),
        icon: "\u{f15c}",
        color: ICON_COLOR_PAIR_NONE,
    },
    Icon {
        m: |f,p| p.is_file(),
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
        let p = self.path();
        FileStat {
            typ: 0 | (if p.is_dir() {FileStat::TYPE_DIR} else {0}) | (if p.is_file() {FileStat::TYPE_FILE} else {0}),
            path: self.path().to_str().unwrap().to_string(),
            file_name: self.file_name().to_str().unwrap().to_string(),
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
        let temp:PathBuf = PathBuf::from(self.path.as_str());
        let parent = temp.parent().unwrap();
        FileStat {
            typ: 0 | (if parent.is_dir() {FileStat::TYPE_DIR} else {0}) | (if parent.is_file() {FileStat::TYPE_FILE} else {0}),
            path: parent.to_str().unwrap().to_string(),
            file_name: parent.file_name().unwrap().to_str().unwrap().to_string(),
        }
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

    let file_watcher = FileWatcher::new(args.nth(1));
    
    let mut selected: i32 = 0;
    let mut scroll: i32 = 0;

    let thread_file_watcher = file_watcher.clone();
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(100));
            let p = thread_file_watcher.path();
            let mut filez = vec![];
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

        let path = file_watcher.path();
        let filez = file_watcher.filez();

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
        }

        if filez.len() > 0 { selected = selected.clamp(0, filez.len() as i32-1); }

        if selected > win.get_max_y()-4+scroll {
            while selected > win.get_max_y()-4+scroll {scroll += 1;}
        }
        if selected < scroll {
            while selected < scroll {scroll -= 1;}
        }
        
        win.refresh();

        match win.getch() {
            Some(Input::Character(c)) => {
                if c == 'q' {
                    break
                }
                if c == '\x08' {
                    let old_path  = file_watcher.path();
                    file_watcher.set_path(Box::new(|path: &mut PathBuf|{
                        path.pop();
                    }));
                    while file_watcher.path2().to_str() == old_path.to_str() { /*thread::sleep(Duration::from_millis(100))*/ }
                    let mut i: usize = 0;
                    for f in file_watcher.filez() {
                        if f.file_name() == old_path.file_name().unwrap() {
                            selected = i as i32;
                            break;
                        }
                        i += 1;
                    }
                }
                if c == '\x0a' {
                    let f: FileStat = file_watcher.filez()[selected as usize].clone();
                    if f.is_dir() {
                        let old_path  = file_watcher.path();
                        file_watcher.set_path(Box::new(move |path: &mut PathBuf|{
                            path.push(f.file_name());
                        }));
                        while file_watcher.path2().to_str() == old_path.to_str() { }
                        scroll = 0;
                        selected = 0;
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
