//! Drain Claude stderr without retaining unbounded output or exposing raw secrets.
use std::sync::{Arc, Mutex};
use tokio::{io::AsyncReadExt, process::ChildStderr, task::JoinHandle};

#[derive(Default)]
struct State {
    tail: Vec<u8>,
    hint: Option<&'static str>,
    seen: bool,
}
impl State {
    fn push(&mut self, bytes: &[u8]) {
        self.seen |= !bytes.is_empty();
        self.tail.extend_from_slice(bytes);
        if self.tail.len() > 8192 {
            self.tail.drain(..self.tail.len() - 8192);
        }
        let text = String::from_utf8_lossy(&self.tail).to_ascii_lowercase();
        // Only fixed, actionable categories leave this buffer. Paths, tokens,
        // prompts and arbitrary tool stderr never reach persisted errors.
        let hint = if text.contains("git-bash")
            || text.contains("git bash")
            || text.contains("claude_code_git_bash_path")
        {
            Some("Windows용 Git Bash 설치와 CLAUDE_CODE_GIT_BASH_PATH 설정을 확인하세요.")
        } else if text.contains("unknown option")
            || text.contains("invalid choice")
            || text.contains("invalid argument")
            || text.contains("invalid value")
        {
            Some("설치된 Claude Code가 실행 옵션을 지원하지 않습니다. CLI 버전을 확인하세요.")
        } else if text.contains("not logged in")
            || text.contains("login required")
            || text.contains("authentication failed")
        {
            Some(
                "Claude Code 로그인이 필요합니다. 같은 사용자 계정에서 claude auth login을 실행하세요.",
            )
        } else if text.contains("no conversation found")
            || text.contains("session id is already in use")
        {
            Some("저장된 Claude 세션을 열 수 없습니다. 원래 세션의 위치와 사용 여부를 확인하세요.")
        } else {
            None
        };
        if hint.is_some() {
            self.hint = hint;
        }
    }
}
pub(super) struct Diagnostics {
    state: Arc<Mutex<State>>,
    reader: JoinHandle<()>,
    joined: bool,
}
impl Diagnostics {
    pub(super) fn start(mut stderr: ChildStderr) -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let capture = state.clone();
        let reader = tokio::spawn(async move {
            let mut buffer = [0; 4096];
            while let Ok(n) = stderr.read(&mut buffer).await {
                if n == 0 {
                    break;
                }
                capture.lock().unwrap().push(&buffer[..n]);
            }
        });
        Self {
            state,
            reader,
            joined: false,
        }
    }
    pub(super) async fn finish(&mut self) {
        // Some descendants may retain the pipe even after the CLI exits.
        if !self.joined
            && tokio::time::timeout(std::time::Duration::from_millis(200), &mut self.reader)
                .await
                .is_ok()
        {
            self.joined = true;
        }
    }
    pub(super) fn hint(&self) -> &'static str {
        let state = self.state.lock().unwrap();
        state.hint.unwrap_or(if state.seen {
            "Claude가 오류 출력을 남겼습니다. 해당 PC의 CLI 설치·실행 환경을 확인하세요."
        } else {
            "Claude Code의 설치·로그인과 실행 환경을 확인하세요."
        })
    }
}
impl Drop for Diagnostics {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostic_capture_is_bounded_and_never_echoes_raw_output() {
        let mut state = State::default();
        state.push(b"error: unknown opt");
        state.push(b"ion --private-secret-option token=do-not-show");
        assert!(state.hint.unwrap().contains("CLI"));
        for _ in 0..100 {
            state.push(&[b'x'; 4096]);
        }
        assert!(state.tail.len() <= 8192);
        assert!(!state.hint.unwrap().contains("do-not-show"));
        state.push(b"Claude Code on Windows requires git-bash");
        assert!(state.hint.unwrap().contains("Git Bash"));
    }
}
