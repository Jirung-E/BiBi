import type {ProviderConfig} from './types';

// Templates only populate an editable draft. They are never registered or selected automatically.
export const providerTemplates = [
 {name:'Codex',adapter:'codex',command:'codex',endpoint:''},
 {name:'Claude Code',adapter:'claude',command:'claude',endpoint:''},
 {name:'Ollama',adapter:'ollama',command:'',endpoint:'http://127.0.0.1:11434'},
] as const satisfies readonly Pick<ProviderConfig,'name'|'adapter'|'command'|'endpoint'>[];
export type ProviderTemplate = typeof providerTemplates[number];
export type ConnectionCheck = {ok:boolean;message:string;models:string[]};
