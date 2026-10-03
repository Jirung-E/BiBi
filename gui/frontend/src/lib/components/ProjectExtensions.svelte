<script lang="ts">
import {onDestroy} from 'svelte';
import {command} from '../api';
import {scrollbars} from '../scrollbars';
import {disclosure,reveal} from '../motion';
import Icon from './Icon.svelte';
import type {Project,ProviderConfig,Host} from '../types';
let {project,providers,hosts}:{project:Project;providers:ProviderConfig[];hosts:Host[]}=$props();
type Kind='skill'|'plugin'|'mcp';
type Entry={kind:Kind;id:string;name:string;description?:string;scope:string;source:unknown;version?:string;enabled:boolean|null;editable:boolean;toggleable:boolean;removable:boolean;content?:string;config?:Record<string,unknown>;note?:string};
type View={supported:boolean;entries:Entry[];errors:string[];notes:string[];revision:string;workspace:string;host_id:string;can_install_plugin:boolean};
let providerId=$state(''),kind=$state<Kind>('skill'),view=$state<View|null>(null),error=$state(''),notice=$state(''),busy=$state(false),loading=$state(false),loaded='';
let editing=$state<Entry|null>(null),adding=$state(false),name=$state(''),body=$state(''),removing=$state(''),checking=$state('');
let checkResult=$state<{id:string;tools:{name:string;description?:string}[]}|null>(null),generation=0,alive=true;
const provider=$derived(providers.find(p=>p.id===providerId));
const entries=$derived(view?.entries.filter(e=>e.kind===kind)??[]);
const title:Record<Kind,string>={skill:'스킬',plugin:'플러그인',mcp:'MCP'};
const canAdd=$derived(!!view?.supported&&(kind!=='plugin'||view.can_install_plugin));
onDestroy(()=>{alive=false;generation++;});
$effect(()=>{const key=project.id+':'+providerId;if(key===loaded)return;loaded=key;view=null;cancel();notice='';if(providerId)void load();});
async function load(){const token=++generation;loading=true;error='';try{const result=await command<View>({type:'list_project_extensions',project_key:project.id,provider_id:providerId});if(alive&&token===generation)view=result;}catch(e){if(alive&&token===generation)error=e instanceof Error?e.message:String(e);}finally{if(alive&&token===generation)loading=false;}}
function cancel(){editing=null;adding=false;name='';body='';removing='';checkResult=null;}
function start(entry?:Entry){editing=entry??null;adding=!entry;name=entry?.id??'';body=entry?.kind==='skill'?entry.content??'':entry?.config?JSON.stringify(entry.config,null,2):kind==='skill'?'---\nname: my-skill\ndescription: \n---\n\n':kind==='mcp'?JSON.stringify({command:'',args:[],env:{}},null,2):'';error='';}
async function change(action:string,id:string,value:unknown=null){if(!view||busy)return;const target=providerId;busy=true;error='';notice='';try{await command({type:'update_project_extension',project_key:project.id,provider_id:target,edit:{kind,action,id,revision:view.revision,value}});if(!alive)return;cancel();notice='저장됨 · 다음 메시지부터 적용';await load();}catch(e){if(alive)error=e instanceof Error?e.message:String(e);}finally{if(alive)busy=false;}}
async function save(){let value:unknown=adding&&kind==='skill'?body.replace('name: my-skill','name: '+name):body;if(kind==='mcp'){try{value=JSON.parse(body);}catch{error='MCP 설정의 JSON 형식을 확인하세요.';return;}}await change(adding?'add':'save',name,value);}
async function check(entry:Entry){if(busy)return;busy=true;checking=entry.id;checkResult=null;error='';try{const result=await command<{tools:{name:string;description?:string}[]}>({type:'check_project_mcp',project_key:project.id,provider_id:providerId,id:entry.id});if(alive)checkResult={id:entry.id,tools:result.tools};}catch(e){if(alive)error=e instanceof Error?e.message:String(e);}finally{if(alive){busy=false;checking='';}}}
function source(entry:Entry){return typeof entry.source==='string'?entry.source:entry.source?JSON.stringify(entry.source):'';}
</script>
<section class="project-extensions" aria-label="프로젝트 확장 관리">
 <div class="extension-target"><strong>{project.name}</strong><label>제공자<span class="extension-select"><select aria-label="확장 제공자" bind:value={providerId} disabled={busy}><option value="">선택</option>{#each providers as p}<option value={p.id}>{p.name} · {hosts.find(h=>h.id===p.host_id)?.name??p.host_id}</option>{/each}</select></span></label></div>
 {#if providerId}<div class="extension-tabs" role="tablist" aria-label="확장 종류">{#each Object.entries(title) as [id,label]}<button role="tab" aria-selected={kind===id} class:active={kind===id} disabled={busy} onclick={()=>{kind=id as Kind;cancel();}}>{label}</button>{/each}<button class="icon-button" aria-label="확장 새로고침" title="새로고침" disabled={busy||loading} onclick={load}><Icon name="refresh" /></button></div>{/if}
 {#if loading}<p role="status">확장 조회 중…</p>{/if}
 {#if error}<p class="error" role="alert">{error}</p>{/if}
 {#if notice}<p class="muted" role="status">{notice}</p>{/if}
 {#if view&&!loading}
  <small class="extension-path">{provider?.host_id==='local'?'이 호스트':hosts.find(h=>h.id===provider?.host_id)?.name??provider?.host_id} · {view.workspace}</small>
  {#each view.errors as message}<p class="error" role="alert">{message}</p>{/each}
  {#if view.notes.length}<details use:disclosure><summary>적용 범위</summary>{#each view.notes as note}<p class="muted">{note}</p>{/each}</details>{/if}
  {#if !adding&&!editing&&canAdd}<div class="form-actions"><button onclick={()=>start()} disabled={busy}><Icon name="plus" />{title[kind]} 추가</button></div>{/if}
  {#if adding||editing}<form class="extension-editor" onsubmit={(e)=>{e.preventDefault();void save();}}>
   {#if adding}<label>{kind==='plugin'?'플러그인 이름@마켓플레이스':'이름'}<input aria-label="확장 이름" bind:value={name} required maxlength="160" disabled={busy} /></label>{:else}<strong>{editing?.name}</strong>{/if}
   {#if kind==='skill'}<label>SKILL.md<textarea use:scrollbars aria-label="스킬 내용" rows="10" bind:value={body} required disabled={busy}></textarea></label>{:else if kind==='mcp'}<label>서버 설정 · JSON<textarea use:scrollbars aria-label="MCP 설정" rows="10" bind:value={body} required disabled={busy} spellcheck="false"></textarea></label><details use:disclosure><summary>설정 형식</summary><code>{provider?.adapter==='codex'?'command · args · env 또는 url · http_headers · bearer_token_env_var':'command · args · env 또는 type: http · url · headers'}</code><p class="muted">__BIBI_KEEP_SECRET__ 값은 저장된 인증 정보를 유지합니다.</p></details>{/if}
   <div class="form-actions"><button type="button" disabled={busy} onclick={cancel}>취소</button><button class="primary" disabled={busy}>{busy?'저장 중…':kind==='plugin'&&adding?'프로젝트에 설치':'저장'}</button></div>
  </form>{:else}
   <div class="extension-list" aria-label={title[kind]+' 목록'}>
    {#each entries as entry(entry.kind+':'+entry.id)}<article class="extension-item">
     <div class="extension-info"><strong>{entry.name}</strong><small>{entry.scope==='project'?'프로젝트':entry.scope==='local'?'프로젝트 로컬':'상속'}{entry.version?' · '+entry.version:''} · {entry.enabled===true?'사용':entry.enabled===false?'끔':'승인 대기'}</small>{#if entry.description}<p>{entry.description}</p>{/if}<details use:disclosure><summary>출처</summary><small class="extension-path">{source(entry)}</small>{#if entry.note}<p>{entry.note}</p>{/if}</details></div>
     <div class="extension-actions">
      {#if entry.toggleable}<button disabled={busy} onclick={()=>change('toggle',entry.id,entry.enabled!==true)} aria-label={entry.name+(entry.enabled===true?' 끄기':' 사용')}>{entry.enabled===true?'끄기':entry.enabled===false?'사용':'사용 승인'}</button>{/if}
      {#if entry.editable}<button disabled={busy} onclick={()=>start(entry)} aria-label={entry.name+' 편집'}>편집</button>{/if}
      {#if kind==='mcp'&&entry.id!=='bibi'}<button disabled={busy} onclick={()=>check(entry)} aria-label={entry.name+' 연결 확인'}>{checking===entry.id?'확인 중…':'연결 확인'}</button>{/if}
      {#if entry.removable}<button disabled={busy} onclick={()=>removing=entry.id} aria-label={entry.name+' 제거'}>제거</button>{/if}
     </div>
     {#if removing===entry.id}<div transition:reveal class="extension-confirm"><span>{entry.name}의 프로젝트 설정을 제거합니다.</span><div class="form-actions"><button disabled={busy} onclick={()=>removing=''}>취소</button><button class="danger-button" disabled={busy} onclick={()=>change('remove',entry.id)}>제거 확인</button></div></div>{/if}
     {#if checkResult?.id===entry.id}<details open use:disclosure><summary>연결됨 · 도구 {checkResult.tools.length}개</summary><ul>{#each checkResult.tools as tool}<li><strong>{tool.name}</strong>{#if tool.description}<p>{tool.description}</p>{/if}</li>{/each}</ul></details>{/if}
    </article>{/each}
    {#if !entries.length&&view.supported&&!view.errors.length}<p class="muted">등록된 {title[kind]} 없음</p>{/if}
   </div>
  {/if}
 {/if}
</section>
<style>
.project-extensions{display:grid;gap:.75rem;min-inline-size:0;}
.extension-select{display:block;min-inline-size:0;overflow:hidden;border-radius:.65rem;}
.extension-select select{inline-size:100%;}
.extension-select:focus-within{outline:.14286rem solid var(--accent);outline-offset:.14286rem;}
.extension-target,.extension-info,.extension-editor{display:grid;gap:.5rem;min-inline-size:0;}
.extension-tabs,.extension-actions{display:flex;gap:.375rem;align-items:center;flex-wrap:wrap;}
.extension-tabs .icon-button{margin-inline-start:auto;}
.extension-tabs .active{color:var(--accent);background:var(--accent-soft);}
.extension-path{overflow-wrap:anywhere;}
.extension-item{display:grid;gap:.625rem;border-block-end:.07143rem solid var(--border);padding-block:.75rem;min-inline-size:0;}
.extension-item p{margin:0;overflow-wrap:anywhere;}
.extension-info small{color:var(--muted);}
.extension-editor textarea{font-family:ui-monospace,SFMono-Regular,Consolas,monospace;min-inline-size:0;max-block-size:40dvh;}
.extension-confirm{border-inline-start:.14286rem solid var(--accent);padding-inline-start:.625rem;}
.extension-list{min-inline-size:0;}
</style>
