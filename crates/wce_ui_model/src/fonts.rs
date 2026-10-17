//! Finding the font a piece of text uses: a state names a font and a size, or a font category (`ui/fontcategories.fc`) that stands for a
//! font, a size and a colour. The font files are `font/<name>_<size>.cuf` in the game's packs.

use std::collections::HashMap;
use std::rc::Rc;

use wce_vfs::Vfs;

use crate::cuf::Cuf;
use crate::dom::Dom;

#[derive(Debug, Clone, PartialEq)]
pub struct Category {
    pub font: String,
    pub size: i32,
    /// RGBA.
    pub color: [u8; 4],
}

/// What text resolves to.
#[derive(Clone)]
pub struct ResolvedFont {
    pub cuf: Rc<Cuf>,
    /// The key of the font file (`la_gioconda_16`), for caches.
    pub key: String,
    /// The colour the category gives, if the text uses one.
    pub color: Option<[u8; 4]>,
}

#[derive(Default)]
pub struct FontLibrary {
    categories: Option<HashMap<String, Category>>,
    fonts: HashMap<String, Option<Rc<Cuf>>>,
    sizes: HashMap<String, Vec<i32>>,
    sizes_ready: bool,
}

pub const DEFAULT_CATEGORY: &str = "Default Font Category";

impl FontLibrary {
    fn load_categories(vfs: &Vfs) -> HashMap<String, Category> {
        let mut m = HashMap::new();
        let Ok(bytes) = vfs.read("ui/fontcategories.fc") else { return m };
        let Ok(x) = wce_ui_format::ui_to_xml(&bytes) else { return m };
        let Ok(dom) = Dom::parse(&x.xml) else { return m };
        for e in dom.root.elems().filter(|e| e.name == "fcentry") {
            let ss: Vec<String> = e.elems().filter(|c| c.name == "s").map(|c| c.text()).collect();
            let us: Vec<i32> = e.elems().filter(|c| c.name == "u").filter_map(|c| c.text().trim().parse().ok()).collect();
            let bs: Vec<u8> = e.elems().filter(|c| c.name == "byte").filter_map(|c| c.text().trim().parse().ok()).collect();
            if ss.len() >= 2 && us.len() >= 2 && bs.len() >= 4 {
                m.insert(ss[0].clone(), Category { font: ss[1].clone(), size: us[1], color: [bs[2], bs[1], bs[0], bs[3]] });
            }
        }

        m
    }

    fn load_sizes(&mut self, vfs: &Vfs) {
        if self.sizes_ready {
            return;
        }

        self.sizes_ready = true;
        for p in vfs.list("font/", ".cuf") {
            let stem = p.trim_start_matches("font/").trim_end_matches(".cuf");
            if let Some((name, size)) = stem.rsplit_once('_') {
                if let Ok(n) = size.parse::<i32>() {
                    self.sizes.entry(name.to_string()).or_default().push(n);
                }
            }
        }

        for v in self.sizes.values_mut() {
            v.sort_unstable();
        }
    }

    /// The category's definition, if there is one of that name.
    pub fn category(&mut self, vfs: &Vfs, name: &str) -> Option<Category> {
        self.categories.get_or_insert_with(|| Self::load_categories(vfs)).get(name).cloned()
    }

    fn font_file(&mut self, vfs: &Vfs, name: &str, size: i32) -> Option<(Rc<Cuf>, String)> {
        self.load_sizes(vfs);
        let mut family = name.to_lowercase();
        if !self.sizes.contains_key(&family) {
            family = "la_gioconda".to_string();
        }

        let avail = self.sizes.get(&family)?;
        let pick = *avail.iter().min_by_key(|s| ((**s - size).abs(), **s))?;
        let key = format!("{family}_{pick}");
        let cuf = self.fonts.entry(key.clone()).or_insert_with(|| vfs.read(&format!("font/{key}.cuf")).ok().and_then(|b| Cuf::parse(&b).ok()).map(Rc::new)).clone()?;
        Some((cuf, key))
    }

    /// The font for text: a category other than the default one decides font, size and colour; otherwise the state's font and size.
    pub fn resolve(&mut self, vfs: &Vfs, font: &str, size: i32, category: &str) -> Option<ResolvedFont> {
        if !category.is_empty() && category != DEFAULT_CATEGORY {
            if let Some(c) = self.category(vfs, category) {
                let (cuf, key) = self.font_file(vfs, &c.font, c.size)?;
                return Some(ResolvedFont { cuf, key, color: Some(c.color) });
            }
        }

        let (cuf, key) = self.font_file(vfs, font, size)?;
        Some(ResolvedFont { cuf, key, color: None })
    }
}
