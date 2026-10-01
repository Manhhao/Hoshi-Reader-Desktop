use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use crate::sync::client::{self, GoogleDriveFile};
use crate::sync::gdrive::handler as drive;
use crate::sync::gdrive::manager::{book_folder, shared};
use crate::sync::model::SyncResult;

#[derive(Default)]
pub(super) struct Listing {
    listed: bool,
    books: Mutex<HashMap<String, String>>,
    folders: Mutex<HashMap<(String, i64), String>>,
    created: Mutex<HashSet<String>>,
    files: HashMap<(String, String), GoogleDriveFile>,
    pub published: AtomicBool,
}

impl Listing {
    pub async fn list() -> SyncResult<Self> {
        let listed = drive::list("'me' in owners").await?;
        let book_folder = book_folder();
        let keys: HashMap<&str, &str> = listed
            .iter()
            .filter(|file| {
                file.parents
                    .as_ref()
                    .is_some_and(|parents| parents.contains(&book_folder))
            })
            .map(|file| (file.id.as_str(), file.name.as_str()))
            .collect();
        let mut books = HashMap::new();
        let mut folders = HashMap::new();
        let mut files = HashMap::new();
        for file in &listed {
            if keys.contains_key(file.id.as_str()) {
                books.entry(file.name.clone()).or_insert(file.id.clone());
            }
            let Some(parent) = file.parents.as_ref().and_then(|parents| parents.first()) else {
                continue;
            };
            match (keys.get(parent.as_str()), file.name.parse::<i64>()) {
                (Some(key), Ok(generation)) if file.is_folder() => {
                    folders
                        .entry((key.to_string(), generation))
                        .or_insert(file.id.clone());
                }
                _ => {
                    files
                        .entry((parent.clone(), file.name.clone()))
                        .or_insert(file.clone());
                }
            }
        }
        shared().cache.book_folders = folders
            .iter()
            .map(|((key, generation), id)| (cache_key(key, *generation), id.clone()))
            .collect();
        Ok(Listing {
            listed: true,
            books: Mutex::new(books),
            folders: Mutex::new(folders),
            files,
            ..Default::default()
        })
    }

    pub async fn folder(&self, key: &str, generation: i64) -> SyncResult<Option<String>> {
        Ok(self.resolve(key, generation).await?.1)
    }

    pub fn forget(&self, key: &str, generation: i64) {
        self.folders
            .lock()
            .unwrap()
            .remove(&(key.to_string(), generation));
        shared()
            .cache
            .book_folders
            .remove(&cache_key(key, generation));
    }

    pub async fn upload(
        &self,
        key: &str,
        generation: i64,
        name: &str,
        data: Vec<u8>,
    ) -> SyncResult<()> {
        let (book, folder) = self.resolve(key, generation).await?;
        let folder = match folder {
            Some(folder) => folder,
            None => {
                let book = match book {
                    Some(book) => book,
                    None => {
                        let book = drive::create_folder(&book_folder(), key).await?;
                        self.books
                            .lock()
                            .unwrap()
                            .insert(key.to_string(), book.clone());
                        book
                    }
                };
                let folder = drive::create_folder(&book, &generation.to_string()).await?;
                self.created.lock().unwrap().insert(folder.clone());
                self.remember(key, generation, &folder);
                folder
            }
        };
        if self.file(&folder, name).is_some() {
            return Ok(());
        }
        if self.listed || self.created.lock().unwrap().contains(&folder) {
            client::write(data, name, &folder, None, "application/octet-stream").await?;
        } else {
            drive::upload(data, name, &folder).await?;
        }
        Ok(())
    }

    pub async fn find(
        &self,
        key: &str,
        generation: i64,
        name: &str,
    ) -> SyncResult<Option<GoogleDriveFile>> {
        let listed = self
            .folders
            .lock()
            .unwrap()
            .get(&(key.to_string(), generation))
            .cloned();
        let cached = shared()
            .cache
            .book_folders
            .get(&cache_key(key, generation))
            .cloned();
        if let Some(folder) = listed.or(cached) {
            let file = match self.file(&folder, name) {
                Some(file) => Some(file),
                None => drive::children(&folder, Some(name))
                    .await?
                    .into_iter()
                    .next(),
            };
            if file.is_some() {
                return Ok(file);
            }
        }
        let generation_name = generation.to_string();
        for book in drive::children(&book_folder(), Some(key)).await? {
            for folder in drive::children(&book.id, Some(&generation_name)).await? {
                let file = drive::children(&folder.id, Some(name))
                    .await?
                    .into_iter()
                    .next();
                if file.is_some() {
                    self.remember(key, generation, &folder.id);
                    return Ok(file);
                }
            }
        }
        Ok(None)
    }

    async fn resolve(
        &self,
        key: &str,
        generation: i64,
    ) -> SyncResult<(Option<String>, Option<String>)> {
        let folder = self
            .folders
            .lock()
            .unwrap()
            .get(&(key.to_string(), generation))
            .cloned();
        let book = self.books.lock().unwrap().get(key).cloned();
        if folder.is_some() || self.listed {
            return Ok((book, folder));
        }
        let book = match book {
            Some(book) => Some(book),
            None => drive::folder(&book_folder(), key, false).await?,
        };
        let folder = match &book {
            Some(book) => drive::folder(book, &generation.to_string(), false).await?,
            None => None,
        };
        if let Some(book) = &book {
            self.books
                .lock()
                .unwrap()
                .insert(key.to_string(), book.clone());
        }
        if let Some(folder) = &folder {
            self.remember(key, generation, folder);
        }
        Ok((book, folder))
    }

    fn remember(&self, key: &str, generation: i64, folder: &str) {
        self.folders
            .lock()
            .unwrap()
            .insert((key.to_string(), generation), folder.to_string());
        shared()
            .cache
            .book_folders
            .insert(cache_key(key, generation), folder.to_string());
    }

    fn file(&self, folder: &str, name: &str) -> Option<GoogleDriveFile> {
        self.files
            .get(&(folder.to_string(), name.to_string()))
            .cloned()
    }
}

fn cache_key(key: &str, generation: i64) -> String {
    format!("{key}/{generation}")
}
