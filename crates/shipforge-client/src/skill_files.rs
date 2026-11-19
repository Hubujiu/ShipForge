use serde_json::{Value, json};
use shipforge_core::skill_package::{self, MAX_FILE_BYTES, MAX_FILES};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{Cursor, Read},
    path::Path,
};
use url::Url;
type Result<T> = std::result::Result<T, String>;
const TOTAL: usize = 2 * 1024 * 1024;
fn io(e: impl std::fmt::Display) -> String {
    e.to_string()
}
pub fn checked_meta(path: &Path) -> Result<fs::Metadata> {
    let meta = fs::symlink_metadata(path).map_err(io)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err("skill source contains a reparse point".into());
        }
    }
    if meta.file_type().is_symlink() || (!meta.is_dir() && !meta.is_file()) {
        return Err("skill source contains an unsupported entry".into());
    }
    Ok(meta)
}
fn bytes(reader: impl Read, limit: usize) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut data)
        .map_err(io)?;
    if data.len() > limit {
        return Err("skill data exceeds import limit".into());
    }
    Ok(data)
}
struct Tree {
    files: BTreeMap<String, String>,
    names: HashSet<String>,
    size: usize,
    embedded: Vec<Value>,
}
impl Tree {
    fn new() -> Self {
        Self {
            files: BTreeMap::new(),
            names: HashSet::new(),
            size: 0,
            embedded: Vec::new(),
        }
    }
    fn push(&mut self, path: String, reader: impl Read) -> Result<()> {
        skill_package::relative(&path)?;
        if self.files.len() >= MAX_FILES * 4 || !self.names.insert(path.to_lowercase()) {
            return Err("too many files or duplicate skill path".into());
        }
        let data = bytes(reader, MAX_FILE_BYTES.min(TOTAL - self.size))?;
        self.size += data.len();
        self.files.insert(
            path,
            String::from_utf8(data).map_err(|_| "skill files must be UTF-8 text")?,
        );
        Ok(())
    }
    fn packages(&self) -> Result<Vec<Value>> {
        let roots = self
            .files
            .keys()
            .filter_map(|s| {
                if s == "SKILL.md" {
                    Some("")
                } else {
                    s.strip_suffix("/SKILL.md")
                }
            })
            .collect::<Vec<_>>();
        if roots.len() + self.embedded.len() == 0 || roots.len() + self.embedded.len() > 16 {
            return Err("expected 1 to 16 SKILL.md packages".into());
        }
        let mut grouped: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
        for (p, text) in &self.files {
            let (root, name) = roots
                .iter()
                .filter_map(|r| {
                    if r.is_empty() {
                        Some((*r, p.as_str()))
                    } else {
                        p.strip_prefix(r)
                            .and_then(|p| p.strip_prefix('/'))
                            .map(|p| (*r, p))
                    }
                })
                .max_by_key(|(r, _)| r.len())
                .ok_or("file is outside every skill package")?;
            grouped
                .entry(root)
                .or_default()
                .push(json!({"path":name,"text":text}));
        }
        let mut out = self.embedded.clone();
        for files in grouped.into_values() {
            let files = json!(files);
            skill_package::validate(&files)?;
            out.push(json!({"files":files}));
        }
        Ok(out)
    }
    fn walk(
        &mut self,
        dir: &Path,
        root: &Path,
        depth: usize,
        visited: &mut usize,
        inside: bool,
    ) -> Result<()> {
        if depth > 12 {
            return Err("skill directory is too deep".into());
        }
        let inside = inside || dir.join("SKILL.md").is_file();
        for entry in fs::read_dir(dir).map_err(io)? {
            *visited += 1;
            if *visited > 512 {
                return Err("too many skill directory entries".into());
            }
            let path = entry.map_err(io)?.path();
            let relative = path
                .strip_prefix(root)
                .map_err(io)?
                .to_str()
                .ok_or("invalid UTF-8 file name")?
                .replace('\\', "/");
            skill_package::relative(&relative)?;
            let meta = checked_meta(&path)?;
            if meta.is_dir() {
                self.walk(&path, root, depth + 1, visited, inside)?;
            } else if !inside
                && path.extension().and_then(|v| v.to_str()).is_some_and(|v| {
                    v.eq_ignore_ascii_case("zip") || v.eq_ignore_ascii_case("skill")
                })
            {
                let data = bytes(fs::File::open(&path).map_err(io)?, TOTAL - self.size)?;
                let packages = decode(&relative, data)?;
                for package in packages {
                    let length = package["files"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|f| f["text"].as_str())
                        .map(str::len)
                        .sum::<usize>();
                    self.size += length;
                    if self.size > TOTAL || self.embedded.len() >= 16 {
                        return Err("skill collection is too large".into());
                    }
                    self.embedded.push(package);
                }
            } else {
                self.push(relative, fs::File::open(&path).map_err(io)?)?;
            }
        }
        Ok(())
    }
}
fn decode(name: &str, data: Vec<u8>) -> Result<Vec<Value>> {
    let mut tree = Tree::new();
    if name.to_ascii_lowercase().ends_with(".zip") || data.starts_with(b"PK\x03\x04") {
        let mut zip = zip::ZipArchive::new(Cursor::new(data)).map_err(io)?;
        if zip.len() > 512 {
            return Err("too many ZIP entries".into());
        }
        zip.validate_header_consistency().map_err(io)?;
        let mut seen = HashSet::new();
        for i in 0..zip.len() {
            let file = zip.by_index(i).map_err(io)?;
            let path = file.name().map_err(io)?.trim_end_matches('/').to_owned();
            skill_package::relative(&path)?;
            if !seen.insert(path.to_lowercase())
                || file.is_symlink()
                || file.encrypted()
                || file.enclosed_name().is_none()
                || file.size() > MAX_FILE_BYTES as u64
            {
                return Err("invalid, duplicate or oversized ZIP member".into());
            }
            if !file.is_dir() {
                tree.push(path, file)?;
            }
        }
    } else if name.eq_ignore_ascii_case("SKILL.md") || name.to_ascii_lowercase().ends_with(".skill")
    {
        tree.push("SKILL.md".into(), Cursor::new(data))?;
    } else {
        return Err("select SKILL.md, a .skill file, a .zip archive or a skill directory".into());
    }
    tree.packages()
}
pub fn from_path(path: &Path) -> Result<Vec<Value>> {
    let m = checked_meta(path)?;
    if m.is_dir() {
        let mut tree = Tree::new();
        tree.walk(path, path, 0, &mut 0, false)?;
        return tree.packages();
    }
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("invalid skill filename")?;
    decode(name, bytes(fs::File::open(path).map_err(io)?, TOTAL)?)
}
fn public_url(raw: &str) -> Result<Url> {
    let mut u = Url::parse(raw).map_err(io)?;
    if u.scheme() != "https"
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
    {
        return Err(
            "select a public HTTPS GitHub file URL without credentials or query parameters".into(),
        );
    }
    if u.host_str() == Some("github.com") {
        let parts = u
            .path_segments()
            .ok_or("invalid GitHub URL")?
            .collect::<Vec<_>>();
        if parts.len() < 5 || !matches!(parts[2], "blob" | "raw") {
            return Err("select a GitHub file rather than a repository page".into());
        }
        let path = format!("/{}/{}/{}", parts[0], parts[1], parts[3..].join("/"));
        u.set_host(Some("raw.githubusercontent.com")).map_err(io)?;
        u.set_path(&path);
    }
    if !matches!(
        u.host_str(),
        Some("raw.githubusercontent.com" | "gist.githubusercontent.com")
    ) {
        return Err("supported public sources are GitHub files and raw Gist files".into());
    }
    Ok(u)
}
fn read_url(directory: Option<&Path>, url: &Url) -> Result<Vec<u8>> {
    let agent = crate::network::http(directory, url.as_str(), 15)?;
    let mut response = agent
        .get(url.as_str())
        .call()
        .map_err(crate::network::failure)?;
    if response.status() != 200 {
        return Err(format!("skill source returned {}", response.status()));
    }
    bytes(response.body_mut().as_reader(), TOTAL)
}
pub(crate) fn prepare(q: Value, directory: Option<&Path>) -> Result<Value> {
    import_with(q, |url| read_url(directory, url))
}
fn import_with(mut q: Value, fetch: impl FnOnce(&Url) -> Result<Vec<u8>>) -> Result<Value> {
    if q["op"] != "skill.import" {
        return Ok(q);
    }
    if ["path", "url", "files", "packages"]
        .iter()
        .filter(|k| q.get(**k).is_some())
        .count()
        != 1
    {
        return Err("choose one skill source".into());
    }
    let packages = if let Some(path) = q["path"].as_str() {
        from_path(Path::new(path))?
    } else if let Some(raw) = q["url"].as_str() {
        let url = public_url(raw)?;
        let name = url
            .path_segments()
            .and_then(|mut v| v.next_back())
            .ok_or("missing skill filename")?;
        decode(name, fetch(&url)?)?
    } else {
        return Ok(q);
    };
    let obj = q.as_object_mut().unwrap();
    obj.remove("path");
    obj.remove("url");
    obj.insert("packages".into(), json!(packages));
    Ok(q)
}
pub fn metadata(package: &Value) -> Result<Value> {
    let p = skill_package::validate(&package["files"])?;
    Ok(json!({"title":p["title"],"description":p["description"]}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_github_file_reaches_managed_import_with_a_synthetic_response() {
        let q=import_with(json!({"op":"skill.import","url":"https://github.com/example/fixture/blob/v1/SKILL.md"}),|url|{
            assert_eq!(url.as_str(),"https://raw.githubusercontent.com/example/fixture/v1/SKILL.md");
            Ok(b"---\nname: Synthetic\ndescription: >-\n  Two line\n  description\n---\nReview tasks".to_vec())
        }).unwrap();
        assert!(q.get("url").is_none());
        assert_eq!(
            metadata(&q["packages"][0]).unwrap()["description"],
            "Two line description"
        );
        let tmp = tempfile::tempdir().unwrap();
        let mut c = crate::Client::local(tmp.path()).unwrap();
        assert_eq!(c.execute(q).unwrap()["results"][0]["status"], "imported");
        assert!(public_url("https://example.invalid/SKILL.md").is_err());
    }
}
