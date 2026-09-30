//! Parse RPM repodata `primary.xml.gz` into package entries.

use std::io::Read;

use crate::error::{ZlError, ZlResult};

/// A single package entry from RPM repodata (primary.xml).
#[derive(Debug, Clone)]
pub struct RpmEntry {
    pub name: String,
    pub version: String,
    pub release: String,
    pub arch: String,
    pub summary: String,
    pub description: String,
    pub installed_size: u64,
    pub location_href: String,
    pub checksum: Option<String>,
    pub requires: Vec<String>,
    pub provides: Vec<String>,
    pub conflicts: Vec<String>,
}

impl RpmEntry {
    /// Full EVR string (epoch:version-release, epoch omitted if 0)
    pub fn evr(&self) -> String {
        format!("{}-{}", self.version, self.release)
    }
}

/// Parse primary.xml from a reader.
pub fn parse_primary_xml<R: Read>(reader: R) -> ZlResult<Vec<RpmEntry>> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;
    use std::io::BufReader;

    let buf_reader = BufReader::new(reader);
    let mut xml = Reader::from_reader(buf_reader);
    // Text is not trimmed per event: an entity reference splits character data
    // into several events, and per-event trimming would eat the surrounding
    // spaces. The accumulated value is trimmed once, on the closing tag.

    let mut entries = Vec::new();
    let mut buf = Vec::new();

    // State tracking
    let mut in_package = false;
    let mut current = RpmEntry {
        name: String::new(),
        version: String::new(),
        release: String::new(),
        arch: String::new(),
        summary: String::new(),
        description: String::new(),
        installed_size: 0,
        location_href: String::new(),
        checksum: None,
        requires: Vec::new(),
        provides: Vec::new(),
        conflicts: Vec::new(),
    };
    let mut current_tag = String::new();
    // quick-xml splits character data around entity references into several
    // events, so text is accumulated here and flushed on the closing tag.
    let mut text_buf = String::new();
    let mut in_requires = false;
    let mut in_provides = false;
    let mut in_conflicts = false;
    let mut checksum_is_sha256 = false;

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let name_ref = e.name();
                let local = local_name(name_ref.as_ref());
                match local {
                    "package" => {
                        in_package = true;
                        current = RpmEntry {
                            name: String::new(),
                            version: String::new(),
                            release: String::new(),
                            arch: String::new(),
                            summary: String::new(),
                            description: String::new(),
                            installed_size: 0,
                            location_href: String::new(),
                            checksum: None,
                            requires: Vec::new(),
                            provides: Vec::new(),
                            conflicts: Vec::new(),
                        };
                    }
                    "name" | "summary" | "description" | "arch" if in_package => {
                        current_tag = local.to_string();
                        text_buf.clear();
                    }
                    "checksum" if in_package => {
                        current_tag = "checksum".to_string();
                        text_buf.clear();
                        // Check if type="sha256"
                        checksum_is_sha256 = false;
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == "type" {
                                let val = attr.value;
                                if val == "sha256" {
                                    checksum_is_sha256 = true;
                                }
                            }
                        }
                    }
                    "rpm:requires" | "requires" if in_package => {
                        in_requires = true;
                    }
                    "rpm:provides" | "provides" if in_package => {
                        in_provides = true;
                    }
                    "rpm:conflicts" | "conflicts" if in_package => {
                        in_conflicts = true;
                    }
                    _ => {
                        current_tag.clear();
                        text_buf.clear();
                    }
                }
            }
            Ok(Event::Empty(e)) => {
                if !in_package {
                    continue;
                }
                let name_ref = e.name();
                let local = local_name(name_ref.as_ref());
                match local {
                    "version" => {
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                "ver" => {
                                    current.version = attr.value.to_string();
                                }
                                "rel" => {
                                    current.release = attr.value.to_string();
                                }
                                _ => {}
                            }
                        }
                    }
                    "location" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == "href" {
                                current.location_href = attr.value.to_string();
                            }
                        }
                    }
                    "size" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == "installed" {
                                current.installed_size = attr.value.parse().unwrap_or(0);
                            }
                        }
                    }
                    "rpm:entry" | "entry" => {
                        let mut dep_name = String::new();
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == "name" {
                                dep_name = attr.value.to_string();
                            }
                        }
                        if !dep_name.is_empty() && !dep_name.starts_with("rpmlib(") {
                            if in_requires {
                                current.requires.push(dep_name);
                            } else if in_provides {
                                current.provides.push(dep_name);
                            } else if in_conflicts {
                                current.conflicts.push(dep_name);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                if !in_package || current_tag.is_empty() {
                    continue;
                }
                text_buf.push_str(&e.xml10_content());
            }
            Ok(Event::GeneralRef(e)) => {
                if !in_package || current_tag.is_empty() {
                    continue;
                }
                // Entity references are reported separately from text since
                // quick-xml 0.38 and must be resolved by hand.
                match e.resolve_char_ref() {
                    Ok(Some(c)) => text_buf.push(c),
                    _ => {
                        if let Some(s) = quick_xml::escape::resolve_xml_entity(&e) {
                            text_buf.push_str(s);
                        }
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name_ref = e.name();
                let local = local_name(name_ref.as_ref());

                if in_package && local == current_tag {
                    let text = text_buf.trim().to_string();
                    match current_tag.as_str() {
                        "name" => current.name = text,
                        "summary" => current.summary = text,
                        "description" => current.description = text,
                        "arch" => current.arch = text,
                        "checksum" if checksum_is_sha256 => current.checksum = Some(text),
                        _ => {}
                    }
                }

                match local {
                    "package" if in_package => {
                        // Use summary as description if description is empty
                        if current.description.is_empty() {
                            current.description = current.summary.clone();
                        }
                        entries.push(current.clone());
                        in_package = false;
                    }
                    "rpm:requires" | "requires" => in_requires = false,
                    "rpm:provides" | "provides" => in_provides = false,
                    "rpm:conflicts" | "conflicts" => in_conflicts = false,
                    _ => {}
                }
                current_tag.clear();
                text_buf.clear();
            }
            Err(e) => {
                return Err(ZlError::Plugin {
                    plugin: "rpm-repodata".into(),
                    message: format!("XML parse error: {}", e),
                });
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(entries)
}

/// Strip namespace prefix from an XML tag name (e.g., "common:name" → "name")
/// Strip the namespace prefix from an element name (`rpm:entry` -> `entry`).
pub(super) fn local_name(full: &str) -> &str {
    full.rsplit(':').next().unwrap_or(full)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_primary_xml_minimal() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<metadata xmlns="http://linux.duke.edu/metadata/common"
          xmlns:rpm="http://linux.duke.edu/metadata/rpm" packages="1">
  <package type="rpm">
    <name>bash</name>
    <arch>x86_64</arch>
    <version epoch="0" ver="5.2.26" rel="3.fc40"/>
    <checksum type="sha256">abc123</checksum>
    <summary>The GNU Bourne Again shell</summary>
    <description>Bash is a sh-compatible shell.</description>
    <size package="2000000" installed="8000000" archive="9000000"/>
    <location href="Packages/b/bash-5.2.26-3.fc40.x86_64.rpm"/>
    <format>
      <rpm:requires>
        <rpm:entry name="glibc"/>
        <rpm:entry name="ncurses-libs"/>
      </rpm:requires>
      <rpm:provides>
        <rpm:entry name="bash"/>
        <rpm:entry name="/bin/bash"/>
      </rpm:provides>
    </format>
  </package>
</metadata>"#;

        let entries = parse_primary_xml(xml.as_bytes()).unwrap();
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.name, "bash");
        assert_eq!(e.version, "5.2.26");
        assert_eq!(e.release, "3.fc40");
        assert_eq!(e.arch, "x86_64");
        assert_eq!(e.summary, "The GNU Bourne Again shell");
        assert_eq!(e.installed_size, 8000000);
        assert_eq!(e.location_href, "Packages/b/bash-5.2.26-3.fc40.x86_64.rpm");
        assert_eq!(e.checksum, Some("abc123".to_string()));
        assert_eq!(e.requires, vec!["glibc", "ncurses-libs"]);
        assert_eq!(e.provides, vec!["bash", "/bin/bash"]);
    }

    #[test]
    fn test_parse_primary_xml_entity_references() {
        // Entity refs split character data into several events; the parser must
        // reassemble them instead of keeping only the last fragment.
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<metadata xmlns="http://linux.duke.edu/metadata/common" packages="1">
  <package type="rpm">
    <name>gtk3</name>
    <arch>x86_64</arch>
    <version epoch="0" ver="3.24.0" rel="1"/>
    <summary>Widgets &amp; toolkit for X &lt;11&gt;</summary>
    <description>Say &quot;hi&quot; &#65; &amp; goodbye</description>
    <location href="Packages/g/gtk3.rpm"/>
  </package>
</metadata>"#;

        let entries = parse_primary_xml(xml.as_bytes()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].summary, "Widgets & toolkit for X <11>");
        assert_eq!(entries[0].description, "Say \"hi\" A & goodbye");
    }

    #[test]
    fn test_rpm_entry_evr() {
        let e = RpmEntry {
            name: "test".into(),
            version: "1.2.3".into(),
            release: "1.fc40".into(),
            arch: "x86_64".into(),
            summary: String::new(),
            description: String::new(),
            installed_size: 0,
            location_href: String::new(),
            checksum: None,
            requires: vec![],
            provides: vec![],
            conflicts: vec![],
        };
        assert_eq!(e.evr(), "1.2.3-1.fc40");
    }
}
