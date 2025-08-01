use color_eyre::Result;
use fast_down::ProgressEntry;
use rkyv::{Archive, Deserialize, Serialize, rancor::Error};
use std::{env, path::PathBuf, sync::Arc};
use tokio::{fs, sync::Mutex};

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DatabaseEntry {
    pub file_path: String,
    pub file_name: String,
    pub file_size: u64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub progress: Vec<ProgressEntry>,
    pub elapsed: u64,
    pub url: String,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DatabaseInner(Vec<DatabaseEntry>);

#[derive(Debug, Clone)]
pub struct Database {
    inner: Arc<Mutex<DatabaseInner>>,
    db_path: Arc<PathBuf>,
}

impl Database {
    pub async fn new() -> Result<Self> {
        let db_path = env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|p| p.to_owned()))
            .unwrap_or(PathBuf::from("."))
            .join("state.fd");
        if db_path.try_exists()? {
            let bytes = fs::read(&db_path).await?;
            let archived = rkyv::access::<ArchivedDatabaseInner, Error>(&bytes)?;
            let deserialized = rkyv::deserialize::<_, Error>(archived)?;
            return Ok(Self {
                inner: Arc::new(Mutex::new(deserialized)),
                db_path: Arc::new(db_path),
            });
        }
        Ok(Self {
            inner: Arc::new(Mutex::new(DatabaseInner(Vec::new()))),
            db_path: Arc::new(db_path),
        })
    }

    pub async fn init_entry(
        &self,
        file_path: String,
        file_name: String,
        file_size: u64,
        etag: Option<String>,
        last_modified: Option<String>,
        url: String,
    ) -> Result<()> {
        let mut inner = self.inner.lock().await;
        inner.0.retain_mut(|e| e.file_path != file_path);
        inner.0.push(DatabaseEntry {
            file_path,
            file_name,
            file_size,
            etag,
            last_modified,
            url,
            progress: vec![],
            elapsed: 0,
        });
        self.flush(inner.clone()).await
    }

    pub async fn get_entry(&self, file_path: &str) -> Option<DatabaseEntry> {
        self.inner
            .lock()
            .await
            .0
            .iter()
            .find(|entry| entry.file_path == file_path)
            .cloned()
    }

    pub async fn update_entry(
        &self,
        file_path: &str,
        progress: Vec<ProgressEntry>,
        elapsed: u64,
    ) -> Result<()> {
        let mut inner = self.inner.lock().await;
        let pos = inner
            .0
            .iter()
            .position(|entry| entry.file_path == file_path)
            .unwrap();
        inner.0[pos].progress = progress;
        inner.0[pos].elapsed = elapsed;
        self.flush(inner.clone()).await
    }

    pub async fn clean_finished(&self) -> Result<usize> {
        let mut inner = self.inner.lock().await;
        let origin_len = inner.0.len();
        #[allow(clippy::single_range_in_vec_init)]
        inner.0.retain_mut(|e| e.progress != [0..e.file_size]);
        self.flush(inner.clone()).await?;
        Ok(origin_len - inner.0.len())
    }

    async fn flush(&self, data: DatabaseInner) -> Result<()> {
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&data)?;
        fs::write(&*self.db_path, bytes).await?;
        Ok(())
    }
}
