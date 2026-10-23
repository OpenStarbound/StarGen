//! Reads the packed asset files of Starbound (`packed.pak`, OpenStarbound's `opensb.pak`).
//!
//! The format, as `PackedAssetSource` reads it, with every number big endian:
//! `SBAsset6`, the offset of the index, the asset data, then at that offset `INDEX`, the metadata (a map of names to tagged json values) and the index (a map of paths to offset and size).
//! Strings are a variable length number of bytes followed by those bytes.
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct Pak {
    path: PathBuf,
    file: Mutex<File>,
    index: HashMap<String, (u64, u64)>,
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn take(&mut self, len: usize) -> Result<&[u8]> {
        let end = self
            .pos
            .checked_add(len)
            .filter(|end| *end <= self.data.len());
        let Some(end) = end else {
            bail!("the index ends in the middle of an entry")
        };
        let bytes = &self.data[self.pos..end];
        self.pos = end;
        Ok(bytes)
    }

    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }

    /// variable length number, seven bits per byte with the highest bits first
    fn vlq(&mut self) -> Result<u64> {
        let mut value = 0u64;
        for _ in 0..10 {
            let byte = self.byte()?;
            value = (value << 7) | (byte & 0x7f) as u64;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        bail!("a variable length number is too long")
    }

    fn string(&mut self) -> Result<&str> {
        let len = self.vlq()? as usize;
        Ok(std::str::from_utf8(self.take(len)?)?)
    }

    /// skips a json value, which is a type (one more than the type of `Json`) followed by what the type has
    fn skip_json(&mut self) -> Result<()> {
        match self.byte()? {
            // null
            0 | 1 => {}
            // float
            2 => {
                self.take(8)?;
            }
            // bool
            3 => {
                self.take(1)?;
            }
            // int
            4 => {
                self.vlq()?;
            }
            // string
            5 => {
                self.string()?;
            }
            // array
            6 => {
                for _ in 0..self.vlq()? {
                    self.skip_json()?;
                }
            }
            // object
            7 => {
                for _ in 0..self.vlq()? {
                    self.string()?;
                    self.skip_json()?;
                }
            }
            other => bail!("unknown json type {other} in the metadata"),
        }
        Ok(())
    }
}

impl Pak {
    pub fn open(path: &Path) -> Result<Pak> {
        let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let mut header = [0u8; 16];
        file.read_exact(&mut header)?;
        if &header[..8] != b"SBAsset6" {
            bail!("{} is not a packed assets file", path.display());
        }
        let index_start = u64::from_be_bytes(header[8..].try_into().unwrap());
        file.seek(SeekFrom::Start(index_start))?;
        let mut tail = Vec::new();
        file.read_to_end(&mut tail)?;

        let mut cursor = Cursor {
            data: &tail,
            pos: 0,
        };
        if cursor.take(5)? != b"INDEX" {
            bail!("{} has no index", path.display());
        }
        for _ in 0..cursor.vlq()? {
            cursor.string()?;
            cursor.skip_json()?;
        }
        let count = cursor.vlq()? as usize;
        let mut index = HashMap::with_capacity(count);
        for _ in 0..count {
            let name = cursor.string()?.to_owned();
            let offset = cursor.u64()?;
            let size = cursor.u64()?;
            index.insert(name, (offset, size));
        }
        Ok(Pak {
            path: path.to_owned(),
            file: Mutex::new(file),
            index,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.index.keys().map(String::as_str)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.index.contains_key(name)
    }

    /// the first `len` bytes of an asset (fewer if it is smaller)
    pub fn read_prefix(&self, name: &str, len: u64) -> Result<Vec<u8>> {
        let &(offset, size) = self
            .index
            .get(name)
            .with_context(|| format!("{name} is not in {}", self.path.display()))?;
        let mut bytes = vec![0; len.min(size) as usize];
        let mut file = self.file.lock().unwrap();
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        self.read_prefix(name, u64::MAX)
    }
}

/// A directory of assets, like OpenStarbound's `assets/opensb`
struct Directory {
    root: PathBuf,
}

impl Directory {
    fn open(root: &Path) -> Result<(Directory, Vec<String>)> {
        fn walk(dir: &Path, prefix: &str, names: &mut Vec<String>) -> Result<()> {
            for entry in
                std::fs::read_dir(dir).with_context(|| format!("listing {}", dir.display()))?
            {
                let entry = entry?;
                let name = format!("{prefix}/{}", entry.file_name().to_string_lossy());
                if entry.file_type()?.is_dir() {
                    walk(&entry.path(), &name, names)?;
                } else {
                    names.push(name);
                }
            }
            Ok(())
        }
        let mut names = Vec::new();
        walk(root, "", &mut names)?;
        Ok((
            Directory {
                root: root.to_owned(),
            },
            names,
        ))
    }

    fn read_prefix(&self, name: &str, len: u64) -> Result<Vec<u8>> {
        let path = self.root.join(name.trim_start_matches('/'));
        let mut bytes = Vec::new();
        File::open(&path)
            .with_context(|| format!("opening {}", path.display()))?
            .take(len)
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}

enum Source {
    Pak(Pak),
    Directory(Directory),
}

/// Several packs and directories laid over each other, the way the game loads them: a later one replaces the assets of an earlier one
pub struct Assets {
    sources: Vec<Source>,
    /// path to the source that has it
    owner: HashMap<String, usize>,
}

impl Assets {
    /// each path is a packed assets file or a directory
    pub fn open(paths: &[PathBuf]) -> Result<Assets> {
        let mut sources = Vec::new();
        let mut owner = HashMap::new();
        for path in paths {
            if path.is_dir() {
                let (directory, names) = Directory::open(path)?;
                for name in names {
                    owner.insert(name, sources.len());
                }
                sources.push(Source::Directory(directory));
            } else {
                let pak = Pak::open(path)?;
                for name in pak.paths() {
                    owner.insert(name.to_owned(), sources.len());
                }
                sources.push(Source::Pak(pak));
            }
        }
        Ok(Assets { sources, owner })
    }

    /// `STARBOUND_PAK` and `OPENSB_PAK` if they are set, else the packs of OpenStarbound's data directory
    pub fn open_default() -> Result<Assets> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let dir = home.join(".local/share/OpenStarbound/assets");
        let packed =
            std::env::var_os("STARBOUND_PAK").map_or_else(|| dir.join("packed.pak"), PathBuf::from);
        let mut paks = vec![packed];
        let opensb =
            std::env::var_os("OPENSB_PAK").map_or_else(|| dir.join("opensb.pak"), PathBuf::from);
        if opensb.exists() {
            paks.push(opensb);
        }
        Assets::open(&paks)
    }

    pub fn len(&self) -> usize {
        self.owner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.owner.is_empty()
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.owner.keys().map(String::as_str)
    }

    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        self.read_prefix(name, u64::MAX)
    }

    pub fn read_prefix(&self, name: &str, len: u64) -> Result<Vec<u8>> {
        let source = self
            .owner
            .get(name)
            .with_context(|| format!("{name} is not an asset"))?;
        match &self.sources[*source] {
            Source::Pak(pak) => pak.read_prefix(name, len),
            Source::Directory(directory) => directory.read_prefix(name, len),
        }
    }
}

/// The size of a png, from the header at its start
pub fn png_size(header: &[u8]) -> Option<(u32, u32)> {
    if header.len() < 24 || &header[..8] != b"\x89PNG\r\n\x1a\n" || &header[12..16] != b"IHDR" {
        return None;
    }
    Some((
        u32::from_be_bytes(header[16..20].try_into().unwrap()),
        u32::from_be_bytes(header[20..24].try_into().unwrap()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vlq(mut value: u64) -> Vec<u8> {
        let mut bytes = vec![(value & 0x7f) as u8];
        value >>= 7;
        while value > 0 {
            bytes.push((value & 0x7f) as u8 | 0x80);
            value >>= 7;
        }
        bytes.reverse();
        bytes
    }

    fn string(text: &str) -> Vec<u8> {
        let mut bytes = vlq(text.len() as u64);
        bytes.extend(text.as_bytes());
        bytes
    }

    /// builds a pack the way `PackedAssetSource::build` does
    fn build(assets: &[(&str, &[u8])]) -> Vec<u8> {
        let mut data = b"SBAsset6".to_vec();
        data.extend([0; 8]);
        let mut index = Vec::new();
        for (name, contents) in assets {
            index.push((name.to_string(), data.len() as u64, contents.len() as u64));
            data.extend(*contents);
        }
        let start = data.len() as u64;
        data.extend(b"INDEX");
        // metadata: one entry of each kind of json value that has a payload
        data.extend(vlq(3));
        data.extend(string("name"));
        data.extend([5]);
        data.extend(string("pack"));
        data.extend(string("list"));
        data.extend([6]);
        data.extend(vlq(3));
        data.extend([4, 0x82, 0x01]); // an int
        data.extend([2]); // a float
        data.extend(1.5f64.to_be_bytes());
        data.extend([1]); // null
        data.extend(string("object"));
        data.extend([7]);
        data.extend(vlq(1));
        data.extend(string("flag"));
        data.extend([3, 1]);
        data.extend(vlq(index.len() as u64));
        for (name, offset, size) in &index {
            data.extend(string(name));
            data.extend(offset.to_be_bytes());
            data.extend(size.to_be_bytes());
        }
        data[8..16].copy_from_slice(&start.to_be_bytes());
        data
    }

    #[test]
    fn reads_what_the_builder_wrote() {
        let dir = std::env::temp_dir().join(format!("stargen-pak-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let big = vec![7u8; 300];
        let first = dir.join("first.pak");
        std::fs::write(&first, build(&[("/a.png", b"hello"), ("/dir/b.png", &big)])).unwrap();
        let second = dir.join("second.pak");
        std::fs::write(&second, build(&[("/a.png", b"over")])).unwrap();

        let pak = Pak::open(&first).unwrap();
        assert_eq!(pak.len(), 2);
        assert_eq!(pak.read("/a.png").unwrap(), b"hello");
        assert_eq!(pak.read("/dir/b.png").unwrap(), big);
        assert_eq!(pak.read_prefix("/dir/b.png", 10).unwrap(), &big[..10]);
        assert!(pak.read("/missing.png").is_err());

        let assets = Assets::open(&[first, second]).unwrap();
        assert_eq!(assets.len(), 2);
        assert_eq!(
            assets.read("/a.png").unwrap(),
            b"over",
            "a later pack replaces an earlier one"
        );
        assert_eq!(assets.read("/dir/b.png").unwrap(), big);

        // a directory goes over the packs the same way
        let loose = dir.join("loose");
        std::fs::create_dir_all(loose.join("dir")).unwrap();
        std::fs::write(loose.join("dir/b.png"), b"loose").unwrap();
        let assets = Assets::open(&[dir.join("first.pak"), loose]).unwrap();
        assert_eq!(assets.read("/dir/b.png").unwrap(), b"loose");
        assert_eq!(assets.read_prefix("/dir/b.png", 3).unwrap(), b"loo");
        assert_eq!(assets.read("/a.png").unwrap(), b"hello");

        // a directory goes over the packs the same way
        let loose = dir.join("loose");
        std::fs::create_dir_all(loose.join("dir")).unwrap();
        std::fs::write(loose.join("dir/b.png"), b"loose").unwrap();
        let assets = Assets::open(&[dir.join("first.pak"), loose]).unwrap();
        assert_eq!(assets.read("/dir/b.png").unwrap(), b"loose");
        assert_eq!(assets.read_prefix("/dir/b.png", 3).unwrap(), b"loo");
        assert_eq!(assets.read("/a.png").unwrap(), b"hello");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_other_files() {
        let path = std::env::temp_dir().join(format!("stargen-notpak-{}", std::process::id()));
        std::fs::write(&path, b"not a pack at all, no").unwrap();
        assert!(Pak::open(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn reads_png_sizes() {
        let mut header = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        header.extend(43u32.to_be_bytes());
        header.extend(86u32.to_be_bytes());
        assert_eq!(png_size(&header), Some((43, 86)));
        assert_eq!(png_size(&header[..20]), None);
        assert_eq!(png_size(b"GIF89a................."), None);
    }
}
