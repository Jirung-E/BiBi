#!/usr/bin/env python3
import json,os,sys,uuid
from pathlib import Path
def send(v): print(json.dumps(v),flush=True)
def receive(): return json.loads(sys.stdin.readline())
if sys.argv[1:]==['auth','status','--json']:
 send({'loggedIn':True,'subscriptionType':'test'});sys.exit()
assert '--input-format' in sys.argv and '--permission-prompt-tool' in sys.argv
assert '--dangerously-skip-permissions' not in sys.argv and '--continue' not in sys.argv
resume=next((a.split('=',1)[1] for a in sys.argv if a.startswith('--resume=')),None)
session=resume or next(a.split('=',1)[1] for a in sys.argv if a.startswith('--session-id='))
uuid.UUID(session)
model=sys.argv[sys.argv.index('--model')+1] if '--model' in sys.argv else 'fixture-claude'
changes=0
path=Path(session+'.fixture.json')
history=json.loads(path.read_text()) if resume else []
first=receive();assert first['request']['subtype']=='initialize'
send({'type':'control_request','request_id':'mcp-init','request':{'subtype':'mcp_message','server_name':'bibi','message':{'jsonrpc':'2.0','id':1,'method':'initialize','params':{'protocolVersion':'2024-11-05'}}}})
assert receive()['response']['response']['mcp_response']['result']['serverInfo']['name']=='bibi'
send({'type':'control_response','response':{'subtype':'success','request_id':first['request_id'],'response':{'commands':[{'name':'compact','description':'fixture compact'},{'name':'model'}]}}})
for line in sys.stdin:
 value=json.loads(line)
 if value.get('type')=='control_request' and value['request']['subtype']=='set_model':
  requested=value['request']['model'];changes+=1
  if requested=='reject-model':send({'type':'control_response','response':{'subtype':'error','request_id':value['request_id'],'error':'fixture model rejected'}})
  else:
   model=requested or 'fixture-claude'
   send({'type':'control_response','response':{'subtype':'success','request_id':value['request_id'],'response':{}}})
  continue
 if value.get('type')!='user':continue
 question=value['message']['content'];history.append(question);path.write_text(json.dumps(history))
 send({'type':'system','subtype':'init','session_id':session,'model':model,'slash_commands':['compact','model']})
 if 'INTERRUPT_FIXTURE' in question:
  value=receive();assert value['request']['subtype']=='interrupt';send({'type':'result','session_id':session,'is_error':False,'result':'interrupted','usage':{}});continue
 if len(history)==1:
  send({'type':'assistant','uuid':'main-agent','session_id':session,'parent_tool_use_id':None,'message':{'id':'agent-spawn','content':[{'type':'tool_use','id':'agent-1','name':'Agent','input':{'description':'검토 전문가','prompt':'fixture 자료만 검토'}}]}})
  send({'type':'assistant','uuid':'child','session_id':session,'parent_tool_use_id':'agent-1','message':{'id':'child-text','model':'fixture-child-model','content':[{'type':'text','text':'서브에이전트의 별도 답변'}]}})
  send({'type':'user','uuid':'agent-result','session_id':session,'message':{'content':[{'type':'tool_result','tool_use_id':'agent-1','content':'검토 완료'}]}})
  send({'type':'control_request','request_id':'permission','request':{'subtype':'can_use_tool','tool_name':'Bash','input':{'command':'fixture-only'},'tool_use_id':'bash-1'}})
  permission=receive()['response']['response'];assert permission['behavior']=='allow';assert permission['updatedInput']=={'command':'fixture-only'}
  send({'type':'control_request','request_id':'mcp-call','request':{'subtype':'mcp_message','server_name':'bibi','message':{'jsonrpc':'2.0','id':2,'method':'tools/call','params':{'name':'bibi_list_files','arguments':{'path':'.'}}}}})
  assert not receive()['response']['response']['mcp_response']['result']['isError']
 text=json.dumps({'turns':len(history),'model':model,'model_changes':changes,'pid':os.getpid(),'resumed':bool(resume),'first':history[0],'last':question},ensure_ascii=False)
 send({'type':'stream_event','session_id':session,'event':{'type':'message_start','message':{'id':'answer-'+str(len(history))}}})
 send({'type':'stream_event','session_id':session,'event':{'type':'content_block_delta','delta':{'type':'text_delta','text':text}}})
 send({'type':'assistant','session_id':session,'message':{'id':'answer-'+str(len(history)),'content':[{'type':'text','text':text}]}})
 send({'type':'rate_limit_event','session_id':session,'rate_limit_info':{'status':'allowed','rateLimitType':'five_hour','utilization':.46,'resetsAt':1800000000}})
 send({'type':'result','session_id':session,'is_error':False,'result':text,'duration_ms':12,'usage':{'input_tokens':10,'cache_read_input_tokens':30,'output_tokens':5}})
