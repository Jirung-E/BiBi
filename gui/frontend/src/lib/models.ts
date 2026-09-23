import type {ModelHistory,ProviderConfig} from './types';
export function modelSuggestions(provider:ProviderConfig|undefined,history:ModelHistory[]) {
 if(!provider)return [];
 const entries=history.filter(h=>h.provider_id===provider.id).sort((a,b)=>b.uses-a.uses||b.last_used-a.last_used);
 return [...new Set([...entries.map(h=>h.model),...provider.models])];
}
export function recentModels(providerId:string,history:ModelHistory[]) {
 return history.filter(h=>h.provider_id===providerId).sort((a,b)=>b.last_used-a.last_used).slice(0,5);
}
