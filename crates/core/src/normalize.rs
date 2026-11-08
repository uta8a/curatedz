use crate::{Error, Result, config::http_url, hash, id, model::Document};

pub struct Entry {
    pub document: Document,
    pub source_item_id: String,
}

pub fn parse(bytes: &[u8], source_id: &str, source_url: &str) -> Result<Vec<Entry>> {
    let base = http_url(source_url)?;
    let feed = feed_rs::parser::Builder::new()
        .base_uri(Some(source_url))
        .id_generator(|_, _, _| String::new())
        .build()
        .parse(bytes)
        .map_err(|_| Error::Normalize("RSS/Atomを解析できません".into()))?;
    if feed.entries.len() > 1000 {
        return Err(Error::Normalize("フィードが1000件を超えています".into()));
    }
    feed.entries
        .into_iter()
        .map(|entry| {
            let canonical_url = entry
                .links
                .iter()
                .filter(|link| link.rel.as_deref().is_none_or(|r| r == "alternate"))
                .find_map(|link| base.join(&link.href).ok())
                .map(|mut url| {
                    url.set_fragment(None);
                    url.to_string()
                })
                .map(|url| http_url(&url).map(|u| u.to_string()))
                .transpose()?;
            let document_id = match &canonical_url {
                Some(url) => id(&["url", url]),
                None if !entry.id.is_empty() => id(&["source", source_id, &entry.id]),
                None => return Err(Error::Normalize("URLとエントリIDがありません".into())),
            };
            let mut document = Document {
                schema_version: 1,
                document_id,
                canonical_url,
                title: entry
                    .title
                    .map(|text| text.content.trim().to_string())
                    .unwrap_or_default(),
                published_at: entry.published,
                updated_at: entry.updated,
                authors: entry
                    .authors
                    .into_iter()
                    .filter_map(|author| author.name)
                    .collect(),
                summary: entry.summary.map(|text| text.content),
                content_hash: String::new(),
            };
            document.content_hash = hash(&serde_json::to_vec(&document)?);
            Ok(Entry {
                document,
                source_item_id: entry.id,
            })
        })
        .collect()
}
