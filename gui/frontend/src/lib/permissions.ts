import type {ApprovalMode,Provider,Run} from './types';

export function approvalModes(provider:Provider|undefined):ApprovalMode[] {
 return provider==='codex'?['on_request','full_access']:provider==='claude'?['on_request','accept_edits','full_access']:[];
}
export function approvalLabel(mode:ApprovalMode|undefined,provider:Provider|undefined):string {
 if(mode==='full_access')return '전체 접근';
 if(mode==='accept_edits')return '편집 자동 승인';
 return provider==='claude'?'수동 승인':'필요 시 승인';
}
export function approvalDescription(mode:ApprovalMode,provider:Provider|undefined):string {
 if(mode==='full_access')return '파일·명령·네트워크에 전체 접근합니다. 서비스의 조직 정책·명시적 제한은 유지됩니다.';
 if(mode==='accept_edits')return '작업 폴더의 편집·파일 작업은 자동 승인하고, 그 밖의 요청은 확인합니다.';
 return provider==='claude'?'승인이 필요한 도구 요청을 직접 확인합니다.':'작업 폴더에서 실행하고, 추가 권한이 필요하면 승인을 요청합니다.';
}
export function runApprovalLabel(run:Run):string {
 if(run.origin==='external'||run.agent_kind==='subagent')return '원래 실행에서 관리';
 if(run.read_only)return '읽기 전용';
 return approvalModes(run.provider).length?approvalLabel(run.approval_mode,run.provider):'제공자에서 관리';
}
