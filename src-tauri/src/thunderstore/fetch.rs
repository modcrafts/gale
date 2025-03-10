use core::str;
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use eyre::Result;
use indexmap::IndexMap;
use log::{info, warn};
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    game::Game,
    logger,
    prefs::Prefs,
    profile::ModManager,
    thunderstore::{PackageListing, Thunderstore},
    util::cmd::StateMutex,
    NetworkClient,
};

pub(super) async fn fetch_package_loop(app: AppHandle, game: Game) {
    const FETCH_INTERVAL: Duration = Duration::from_secs(60 * 15);

    let manager = app.state::<Mutex<ModManager>>();
    let thunderstore = app.state::<Mutex<Thunderstore>>();
    let prefs = app.state::<Mutex<Prefs>>();

    read_and_insert_cache(manager, thunderstore.clone());

    let mut is_first = true;

    loop {
        let fetch_automatically = prefs.lock().unwrap().fetch_mods_automatically();

        // always fetch once, even if the setting is turned off
        if !fetch_automatically && !is_first {
            info!("automatic fetch cancelled by user setting");
            break;
        };

        if let Err(err) = loop_iter(game, &mut is_first, &app, thunderstore.clone()).await {
            logger::log_webview_err("Error while fetching packages from Thunderstore", err, &app);
        }

        tokio::time::sleep(FETCH_INTERVAL).await;
    }

    async fn loop_iter(
        game: Game,
        is_first: &mut bool,
        app: &AppHandle,
        thunderstore: StateMutex<'_, Thunderstore>,
    ) -> Result<()> {
        if thunderstore.lock().unwrap().is_fetching {
            warn!("automatic fetch cancelled due to ongoing fetch");
            return Ok(());
        }

        let result = fetch_packages(app, game, *is_first).await;

        let mut lock = thunderstore.lock().unwrap();
        lock.is_fetching = false;
        lock.packages_fetched |= result.is_ok();
        *is_first &= result.is_err();

        result
    }
}

fn read_and_insert_cache(manager: StateMutex<ModManager>, state: StateMutex<Thunderstore>) {
    let manager = manager.lock().unwrap();

    match super::read_cache(&manager) {
        Ok(Some(mods)) => {
            let mut thunderstore = state.lock().unwrap();

            for package in mods {
                thunderstore.packages.insert(package.uuid, package);
            }
        }
        Ok(None) => (),
        Err(err) => warn!("failed to read cache: {}", err),
    }
}

const EXCLUDED_PACKAGES_STR: &str = include_str!("../../excluded_packages.txt");

lazy_static! {
    static ref EXCLUDED_PACKAGES: Vec<&'static str> = EXCLUDED_PACKAGES_STR
        .split('\n')
        .map(|line| line.trim())
        .collect();
}

pub(super) async fn fetch_packages(
    app: &AppHandle,
    game: Game,
    write_directly: bool,
) -> Result<()> {
    const UPDATE_INTERVAL: Duration = Duration::from_millis(250);
    const INSERT_EVERY: usize = 1000;

    info!(
        "fetching packages for {}, write_directly: {}",
        game.slug, write_directly
    );

    let state = app.state::<Mutex<Thunderstore>>();
    let client = &app.state::<NetworkClient>().0;

    let primary_url = format!("https://thunderstore.io/c/{}/api/v1/package/", game.slug);
    let mut package_buffer = fetch_and_parse_packages(client, &primary_url).await?;

    if game.slug == "lethal-company" {
        let extra_url = "https://cdn.potatoepet.de/c/lethal-company/api/v1/package/";
        let extra_packages = fetch_and_parse_packages(client, extra_url).await?;
        package_buffer.extend(extra_packages);
    }

    let package_count = package_buffer.len();
    let start_time = Instant::now();
    let mut last_update = Instant::now();

    if write_directly {
        let mut state = state.lock().unwrap();
        state.packages.extend(package_buffer.drain(..));
    } else {
        let mut state = state.lock().unwrap();
        state.packages = package_buffer;
    }

    state.packages_fetched = true;
    state.is_fetching = false;

    info!(
        "fetched {} packages for {} in {:?}",
        package_count, game.slug, start_time.elapsed()
    );

    app.emit("status_update", None::<String>).ok();

    return Ok(());

    async fn fetch_and_parse_packages(client: &reqwest::Client, url: &str) -> Result<IndexMap<String, PackageListing>> {
        let mut response = client.get(url).send().await?.error_for_status()?;
        let mut byte_buffer = Vec::new();
        let mut str_buffer = String::new();
        let mut package_buffer = IndexMap::new();

        while let Some(chunk) = response.chunk().await? {
            byte_buffer.extend_from_slice(&chunk);
            let Ok(chunk) = str::from_utf8(&byte_buffer) else {
                continue;
            };

            str_buffer.push_str(chunk);
            byte_buffer.clear();

            while let Some(index) = str_buffer.find("}]},") {
                let (json, _) = str_buffer.split_at(index + 3);

                match serde_json::from_str::<PackageListing>(json) {
                    Ok(package) => {
                        if !EXCLUDED_PACKAGES.contains(&package.full_name()) {
                            package_buffer.insert(package.uuid.clone(), package);
                        }
                    }
                    Err(err) => warn!("failed to deserialize package: {}", err),
                }
                str_buffer.replace_range(..index + 4, "");
            }
        }
        Ok(package_buffer)
    }
}

pub async fn wait_for_fetch(app: &AppHandle) {
    let thunderstore = app.state::<Mutex<Thunderstore>>();

    loop {
        {
            let thunderstore = thunderstore.lock().unwrap();
            if thunderstore.packages_fetched() {
                return;
            }
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
