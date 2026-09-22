import type { Provider, RunState } from './types';
export const product = { name: 'BiBi', command: 'bibi' };
export const providers: Record<Provider,string> = { codex: 'Codex', claude: 'Claude Code', ollama: 'Ollama', mock: '모의 실행' };
export const states: Record<RunState,string> = { queued: '접수됨', running: '실행 중', waiting_user: '입력 대기', waiting_expert: '전문가 대기', completed: '결과 저장됨', failed: '실패', interrupted: '중단됨', uncertain: '실행 확인 필요', disconnected: '연결 끊김' };
export const shortId = (id: string) => id.split('_').pop()?.slice(0,6) ?? id;
export const age = (time: number|null|undefined, now = Date.now()) => {
  if (!time) return '확인 불가';
  const seconds = Math.max(0, Math.floor((now-time)/1000));
  return seconds < 5 ? '방금' : seconds < 60 ? seconds+'초 전' : seconds < 3600 ? Math.floor(seconds/60)+'분 전' : seconds < 86400 ? Math.floor(seconds/3600)+'시간 전' : Math.floor(seconds/86400)+'일 전';
};
export const dateTime = (time: number|null) => time ? new Date(time).toLocaleString('ko-KR') : '확인 불가';
export const isActive = (state: RunState) => ['running','waiting_user','waiting_expert'].includes(state);
