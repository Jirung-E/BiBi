import type { Event } from './types';
export class ApiError extends Error { constructor(message: string, public status: number) { super(message); } }
export const isDesktop = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
async function invoke<T>(command: string, args?: Record<string,unknown>): Promise<T> {
  return (await import('@tauri-apps/api/core')).invoke<T>(command,args);
}
export async function request<T>(path: string, method = 'GET', body?: unknown): Promise<T> {
  if (isDesktop()) {
    try { return await invoke<T>('api_request', { path, method, body: body ?? null }); }
    catch (error) {
      const e = error as { status?: number; message?: string };
      throw new ApiError(e.message ?? String(error), e.status ?? 0);
    }
  }
  let response: Response;
  try { response = await fetch(path, { method, credentials: 'same-origin', headers: body === undefined ? {} : { 'Content-Type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body) }); }
  catch { throw new ApiError('서버에 연결할 수 없습니다. 초안은 보존됩니다.',0); }
  const data = await response.json().catch(() => ({}));
  if (!response.ok) throw new ApiError(data.error ?? '서버 응답 오류',response.status);
  return data as T;
}
export const command = <T>(body: unknown) => request<T>('/api/command','POST',body);
export const login = (token: string) => request('/auth/login','POST',{token});
export async function connection(): Promise<{mode:string;url:string}> {
  return isDesktop() ? invoke('connection_info') : {mode:'browser',url:location.origin};
}
export async function setConnection(url: string, token: string) { await invoke('set_connection',{url,token}); }
export function subscribe(after: number, onEvent: (event: Event)=>void, onStatus: (connected:boolean)=>void): ()=>void {
  let stopped = false, cursor = after, stream: EventSource|null = null, timer: ReturnType<typeof setTimeout>|undefined;
  const accept = (event: Event) => { if (!stopped && event.seq > cursor) { cursor = event.seq; onEvent(event); } };
  if (isDesktop()) {
    const poll = async () => {
      try { for (const event of await request<Event[]>('/api/events?after='+cursor)) accept(event); if (!stopped) onStatus(true); }
      catch { if (!stopped) onStatus(false); }
      if (!stopped) timer = setTimeout(poll,600);
    };
    void poll();
  } else {
    const connect = () => {
      if (stopped) return;
      stream = new EventSource('/api/stream?after='+cursor);
      stream.onopen = () => onStatus(true);
      stream.addEventListener('update', event => { try { accept(JSON.parse((event as MessageEvent).data)); } catch { onStatus(false); } });
      stream.onerror = () => { onStatus(false); stream?.close(); timer = setTimeout(connect,2000); };
    };
    connect();
  }
  return () => { stopped = true; stream?.close(); clearTimeout(timer); };
}
