use super::*;
use std::collections::{HashMap, HashSet};

impl Store {
    /// Search saved local records only; never contacts a model or returns a transcript.
    pub fn search_sessions(&self, project: &str, query: &str) -> Result<Vec<SessionSearchHit>> {
        let query = query.trim().to_lowercase();
        if query.chars().count() > 160 {
            return Err(Error::Invalid("검색어는 160자 이하로 입력하세요.".into()));
        }
        self.read(|c| {
            let _: Project = required(c, "project", project)?;
            let hidden: HashSet<String> = list(c, "hidden_session", None)?.into_iter().collect();
            let providers: Vec<ProviderConfig> = list(c, "provider", None)?;
            let works: Vec<Work> = list(c, "work", Some(project))?;
            let memberships: Vec<SessionGroups> = list(c, "session_groups", Some(project))?;
            let runs: Vec<Run> = list(c, "run", None)?;
            let runs: Vec<_> = runs.into_iter().filter(|r| r.project_key == project && !hidden.contains(r.session_id())).collect();
            let continued: HashSet<_> = runs.iter().filter_map(|r| r.continued_from.as_deref()).collect();
            let mut latest = HashMap::<String, &Run>::new();
            for run in &runs {
                if !continued.contains(run.id.as_str()) {
                    let entry = latest.entry(run.session_id().into()).or_insert(run);
                    if (run.created_at, run.updated_at, &run.id) > (entry.created_at, entry.updated_at, &entry.id) { *entry = run; }
                }
            }
            let mut messages = c.prepare("SELECT json_extract(data,'$.text') FROM entities WHERE kind='message' AND owner=?1 AND instr(lower(json_extract(data,'$.text')),?2)>0 ORDER BY created_at DESC LIMIT 1")?;
            let mut hits = HashMap::<String, SessionSearchHit>::new();
            for run in &runs {
                let Some(current) = latest.get(run.session_id()) else { continue };
                let provider = providers.iter().find(|p| Some(&p.id) == run.provider_id.as_ref()).map(|p| p.name.as_str()).unwrap_or("");
                let title=works.iter().find(|w|w.id==run.work_id).map(|w|w.title.as_str()).unwrap_or("");
                let groups=memberships.iter().find(|m|m.session_id==run.session_id()).map(|m|works.iter().filter(|w|m.work_ids.contains(&w.id)).map(|w|w.title.as_str()).collect::<Vec<_>>().join(" ")).unwrap_or_default();
                let metadata = format!("{title} {groups} {} {} {} {} {:?} {} {}",run.title,run.model,provider,run.role,run.provider,run.session_id(),run.session_key.as_deref().unwrap_or(""));
                let excerpt = if query.is_empty() || metadata.to_lowercase().contains(&query) {
                    Some(current.context.question.clone())
                } else {
                    messages.query_row(params![run.id,query], |row| row.get::<_,String>(0)).optional()?
                };
                if let Some(text) = excerpt {
                    let chars: Vec<_> = text.chars().collect();
                    let at = text.to_lowercase().find(&query).map(|offset| text.to_lowercase()[..offset].chars().count()).unwrap_or(0);
                    let start = at.saturating_sub(35).min(chars.len());
                    let end = (start + 180).min(chars.len());
                    let excerpt = format!("{}{}{}",if start>0 {"…"} else {""},chars[start..end].iter().collect::<String>(),if end<chars.len(){"…"}else{""});
                    hits.entry(run.session_id().into()).or_insert_with(|| SessionSearchHit {run:(*current).clone(),excerpt});
                }
            }
            let mut hits: Vec<_> = hits.into_values().collect();
            hits.sort_by(|a,b| (b.run.updated_at,&b.run.id).cmp(&(a.run.updated_at,&a.run.id)));
            hits.truncate(100);
            Ok(hits)
        })
    }
}
