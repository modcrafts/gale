use std::{
    borrow::Cow,
    hash::{self, Hash},
};

use heck::{ToKebabCase, ToPascalCase};
use serde::{Deserialize, Serialize};

const JSON: &str = include_str!("../games.json");

lazy_static! {
    static ref GAMES: Vec<GameData<'static>> = serde_json::from_str(JSON).unwrap();
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct JsonGame<'a> {
    name: &'a str,
    #[serde(default)]
    slug: Option<&'a str>,
    #[serde(default)]
    popular: bool,
    mod_loader: ModLoader,
    #[serde(default, rename = "r2dirName")]
    r2_dir_name: Option<&'a str>,
    #[serde(default)]
    extra_sub_dirs: Vec<Subdir<'a>>,
    #[serde(borrow)]
    platforms: Platforms<'a>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum ModLoader {
    BepInEx,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Platforms<'a> {
    #[serde(borrow)]
    steam: Steam<'a>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase", untagged)]
enum Steam<'a> {
    Concise(u32),
    #[serde(rename_all = "camelCase")]
    Full {
        id: u32,
        dir_name: &'a str,
    },
}

#[derive(Serialize, Deserialize, Debug, Eq)]
#[serde(rename_all = "camelCase", from = "JsonGame")]
struct GameData<'a> {
    name: &'a str,
    slug: Cow<'a, str>,
    steam_name: &'a str,
    steam_id: u32,
    mod_loader: ModLoader,
    r2_dir_name: Cow<'a, str>,
    extra_sub_dirs: Vec<Subdir<'a>>,
    popular: bool,
}

impl<'a> From<JsonGame<'a>> for GameData<'a> {
    fn from(value: JsonGame<'a>) -> Self {
        let JsonGame {
            name,
            slug,
            popular,
            mod_loader,
            r2_dir_name,
            extra_sub_dirs,
            platforms,
        } = value;

        let slug = match slug {
            Some(slug) => Cow::Borrowed(slug),
            None => Cow::Owned(name.to_kebab_case()),
        };

        let r2_dir_name = match r2_dir_name {
            Some(name) => Cow::Borrowed(name),
            None => Cow::Owned(slug.to_pascal_case()),
        };

        let (steam_id, steam_name) = match platforms.steam {
            Steam::Concise(id) => (id, name),
            Steam::Full { id, dir_name } => (id, dir_name),
        };

        Self {
            name,
            slug,
            steam_name,
            steam_id,
            mod_loader,
            r2_dir_name,
            extra_sub_dirs,
            popular,
        }
    }
}

impl PartialEq for GameData<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.slug == other.slug
    }
}

impl Hash for GameData<'_> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.slug.hash(state);
    }
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Subdir<'a> {
    name: &'a str,
    target: &'a str,
    /// Whether to separate mods into `author-name` dirs.
    #[serde(default = "default_true")]
    separate_mods: bool,
    #[serde(default = "default_false")]
    mutable: bool,
}

impl<'a> Subdir<'a> {
    pub const fn new(name: &'a str, target: &'a str) -> Self {
        Self {
            name,
            target,
            separate_mods: true,
            mutable: false,
        }
    }

    pub const fn dont_separate_mods(mut self) -> Self {
        self.separate_mods = false;
        self
    }

    pub const fn mutable(mut self) -> Self {
        self.mutable = true;
        self
    }

    pub fn name(&self) -> &'a str {
        self.name
    }

    pub fn target(&self) -> &'a str {
        self.target
    }

    pub fn separate_mods(&self) -> bool {
        self.separate_mods
    }

    pub fn is_mutable(&self) -> bool {
        self.mutable
    }
}

impl ModLoader {
    pub fn default_subdir(&self) -> &'static Subdir<'static> {
        match self {
            ModLoader::BepInEx => {
                const SUBDIR: &Subdir = &Subdir::new("plugins", "BepInEx/plugins");
                SUBDIR
            }
        }
    }

    pub fn subdirs(&self) -> &'static [Subdir<'static>] {
        match self {
            ModLoader::BepInEx => {
                const SUBDIRS: &[Subdir] = &[
                    Subdir::new("plugins", "BepInEx/plugins"),
                    Subdir::new("patchers", "BepInEx/patchers"),
                    Subdir::new("monomod", "BepInEx/monomod"),
                    Subdir::new("core", "BepInEx/core"),
                    Subdir::new("config", "BepInEx/config")
                        .dont_separate_mods()
                        .mutable(),
                ];
                SUBDIRS
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Game(&'static GameData<'static>);

impl Game {
    pub fn all() -> impl Iterator<Item = Self> {
        GAMES.iter().map(Self)
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        GAMES.iter().find(|game| game.slug == slug).map(Self)
    }

    pub fn subdirs(self) -> impl Iterator<Item = &'static Subdir<'static>> {
        self.0
            .mod_loader
            .subdirs()
            .into_iter()
            .chain(self.0.extra_sub_dirs.iter())
    }

    pub fn name(self) -> &'static str {
        self.0.name
    }

    pub fn slug(self) -> &'static str {
        &self.0.slug
    }

    pub fn steam_name(self) -> &'static str {
        self.0.steam_name
    }

    pub fn steam_id(self) -> u32 {
        self.0.steam_id
    }

    pub fn mod_loader(self) -> ModLoader {
        self.0.mod_loader.clone()
    }

    pub fn r2_dir_name(self) -> &'static str {
        &self.0.r2_dir_name
    }

    pub fn is_popular(self) -> bool {
        self.0.popular
    }
}
