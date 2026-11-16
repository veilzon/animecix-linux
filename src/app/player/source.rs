use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Direct(String),
    Embed(String),
}

impl Source {
    pub fn hint_url(&self) -> &str {
        match self {
            Source::Direct(url) | Source::Embed(url) => url,
        }
    }
}

/// Aday URL'leri + embed yedekleri + API kalite etiketlerini tek sırada tutar.
#[derive(Clone, Debug)]
pub struct SourceQueue {
    pub sources: Vec<Source>,
    pub quality: Vec<Option<String>>,
    pub labels: Vec<String>,
    pub index: usize,
}

impl SourceQueue {
    pub fn new(
        candidates: &[String],
        fast_embeds: &[String],
        fallback_embeds: &[String],
        quality_map: &HashMap<String, String>,
        source_labels: &[String],
        fallback_labels: &[String],
    ) -> Self {
        let mut sources = Vec::with_capacity(candidates.len() + fallback_embeds.len());
        let mut quality = Vec::with_capacity(sources.capacity());
        let mut labels = Vec::with_capacity(sources.capacity());
        for (i, c) in candidates.iter().enumerate() {
            let emb = fast_embeds.get(i).cloned();
            quality.push(emb.as_ref().and_then(|e| quality_map.get(e).cloned()));
            labels.push(source_labels.get(i).cloned().unwrap_or_default());
            sources.push(Source::Direct(c.clone()));
        }
        for (i, e) in fallback_embeds.iter().enumerate() {
            quality.push(quality_map.get(e).cloned());
            labels.push(fallback_labels.get(i).cloned().unwrap_or_default());
            sources.push(Source::Embed(e.clone()));
        }
        Self {
            sources,
            quality,
            labels,
            index: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.sources.len()
    }

    pub fn current(&self) -> Option<&Source> {
        self.sources.get(self.index)
    }

    pub fn current_quality(&self) -> Option<&str> {
        self.quality.get(self.index).and_then(|q| q.as_deref())
    }

    pub fn current_label(&self) -> Option<&str> {
        self.labels
            .get(self.index)
            .map(String::as_str)
            .filter(|s| !s.is_empty())
    }

    pub fn next_after_failure(&self) -> Option<usize> {
        let next = self.index + 1;
        (next < self.sources.len()).then_some(next)
    }
}

/// Kaynak değiştirirken konumu koru; 1 saniyenin altını baştan başlat.
pub fn resume_after_switch(pos: f64, _dur: f64) -> Option<f64> {
    (pos >= 1.0 && pos.is_finite()).then_some(pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_prioritizes_direct_then_embed_fallbacks() {
        let q = SourceQueue::new(
            &["https://a/1.mp4".into()],
            &["https://fast/e1".into()],
            &["https://fallback/e2".into()],
            &HashMap::from([("https://fast/e1".into(), "1080p".into())]),
            &["Adonis Fansub".into()],
            &["Adonis Fansub".into()],
        );
        assert_eq!(q.len(), 2);
        assert_eq!(q.quality[0].as_deref(), Some("1080p"));
        assert_eq!(q.labels[1], "Adonis Fansub");
        assert_eq!(q.current_label(), Some("Adonis Fansub"));
        assert_eq!(q.next_after_failure(), Some(1));
        assert_eq!(q.current().map(Source::hint_url), Some("https://a/1.mp4"));
    }

    #[test]
    fn failover_preserves_position_not_zero() {
        assert_eq!(resume_after_switch(123.4, 1440.0), Some(123.4));
        assert_eq!(resume_after_switch(0.5, 1440.0), None);
        assert_eq!(resume_after_switch(-1.0, 1440.0), None);
    }
}
