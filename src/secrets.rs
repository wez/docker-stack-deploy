use anyhow::Context;
use keepass::db::NodeRef;
use keepass::{Database, DatabaseKey};

pub struct KeePassDB {
    db: Database,
}

impl KeePassDB {
    pub fn open_with_password(path: &str, password: &str) -> anyhow::Result<Self> {
        let mut db_file = std::fs::File::open(path)
            .with_context(|| format!("failed to open kdbx file {path}"))?;
        let key = DatabaseKey::new().with_password(password);
        log::debug!("Opening database");
        let db = Database::open(&mut db_file, key)?;
        log::debug!("Database opened");

        Ok(Self { db })
    }

    /// Given a path like "Database/group/group/entryname/fieldname"
    /// returns the string value of the field.
    /// The path elements are case insensitive.
    pub fn resolve_value(&self, path: &str) -> Option<String> {
        fn resolve(parent: NodeRef, path: &[&str]) -> Option<String> {
            let element = path.get(0)?;

            match parent {
                NodeRef::Group(group) => {
                    if !group.name.eq_ignore_ascii_case(*element) {
                        return None;
                    }
                    for child in &group.children {
                        match resolve(child.as_ref(), &path[1..]) {
                            Some(node) => return Some(node),
                            None => {}
                        }
                    }
                    None
                }
                NodeRef::Entry(entry) => {
                    if !entry
                        .get_title()
                        .map(|title| title.eq_ignore_ascii_case(*element))
                        .unwrap_or(false)
                    {
                        return None;
                    }

                    let element = path.get(1)?;

                    if path.len() > 2 {
                        // Path is too long
                        return None;
                    }

                    // We iterate the elements so that we can do a case
                    // insensitive comparison
                    for k in entry.fields.keys() {
                        if k.eq_ignore_ascii_case(*element) {
                            return entry.get(k).map(|s| s.to_string());
                        }
                    }

                    None
                }
            }
        }

        let elements: Vec<&str> = path.split('/').collect();
        resolve(NodeRef::Group(&self.db.root), &elements)
    }
}
