use super::*;

fn sorted(mut ids: Vec<String>) -> Vec<String> {
    ids.sort();
    ids.dedup();
    ids
}

impl Store {
    // A missing record is the legacy single-group membership. An explicitly
    // empty record means ungrouped; neither case rewrites the run or work.
    pub fn set_session_groups(
        &self,
        run_id: &str,
        work_ids: Vec<String>,
        expected_work_ids: Vec<String>,
    ) -> Result<SessionGroups> {
        if work_ids.len() > 256 || expected_work_ids.len() > 256 {
            return Err(Error::Invalid(
                "세션에 연결할 수 있는 그룹은 256개까지입니다.".into(),
            ));
        }
        let work_ids = sorted(work_ids);
        let expected = sorted(expected_work_ids);
        self.write(|c| {
            let run: Run = required(c, "run", run_id)?;
            for id in &work_ids {
                let work: Work = required(c, "work", id)?;
                if work.project_key != run.project_key {
                    return Err(Error::Conflict(
                        "같은 프로젝트의 그룹에만 연결할 수 있습니다.".into(),
                    ));
                }
            }
            let current: SessionGroups = get(c, "session_groups", run.session_id())?
                .unwrap_or_else(|| SessionGroups {
                    session_id: run.session_id().into(),
                    work_ids: vec![run.work_id.clone()],
                });
            if current.work_ids == work_ids {
                return Ok(current);
            }
            if current.work_ids != expected {
                return Err(Error::Conflict(
                    "다른 화면에서 그룹 연결이 변경되었습니다. 닫았다가 다시 열어 확인하세요."
                        .into(),
                ));
            }
            let groups = SessionGroups {
                session_id: run.session_id().into(),
                work_ids,
            };
            put(
                c,
                "session_groups",
                run.session_id(),
                &run.project_key,
                now(),
                &groups,
            )?;
            emit(c, "session_groups", &groups)?;
            Ok(groups)
        })
    }
}

// Native subagent discovery can join previously separate observations. Keep
// the union of their visible memberships under the canonical session identity.
// Old records remain valid for any history not included in this reconciliation.
pub(super) fn reconcile(c: &Connection, children: &[Run], session: &str) -> Result<()> {
    let mut explicit = false;
    let mut ids = Vec::new();
    for child in children {
        if let Some(groups) = get::<SessionGroups>(c, "session_groups", child.session_id())? {
            explicit = true;
            ids.extend(groups.work_ids);
        } else {
            ids.push(child.work_id.clone());
        }
    }
    if !explicit {
        return Ok(());
    }
    let groups = SessionGroups {
        session_id: session.into(),
        work_ids: sorted(ids),
    };
    if get::<SessionGroups>(c, "session_groups", session)?.as_ref() != Some(&groups) {
        put(
            c,
            "session_groups",
            session,
            &children[0].project_key,
            now(),
            &groups,
        )?;
        emit(c, "session_groups", &groups)?;
    }
    Ok(())
}
