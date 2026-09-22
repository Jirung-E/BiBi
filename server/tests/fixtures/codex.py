#!/usr/bin/env python3
import json,sys,os,uuid
from pathlib import Path
session=None;history=[];resumed=False

def send(v):print(json.dumps(v),flush=True)
def reply(v,r):send({'id':v['id'],'result':r})
def event(method,params):send({'method':method,'params':params})
for line in sys.stdin:
 v=json.loads(line);method=v.get('method');p=v.get('params',{})
 if method=='initialize':reply(v,{})
 elif method=='initialized':pass
 elif method in ('thread/start','thread/resume'):
  cwd=Path(p['cwd']);resumed=method=='thread/resume';session=p['threadId'] if resumed else str(uuid.uuid4());path=cwd/(session+'.codex-fixture.json');history=json.loads(path.read_text()) if resumed else []
  if not resumed:assert p['dynamicTools']
  reply(v,{'thread':{'id':session}})
 elif method=='turn/start':
  assert p['threadId']==session
  history.append(p['input'][0]['text']);path.write_text(json.dumps(history));turn='turn-'+str(len(history));reply(v,{'turn':{'id':turn}})
  if len(history)==1:event('item/completed',{'threadId':session,'item':{'id':'spawn','type':'collabAgentToolCall','tool':'spawnAgent','senderThreadId':session,'receiverThreadIds':['child-native'],'prompt':'test child','agentsStates':{'child-native':{'status':'running'}}}})
  result=json.dumps({'turns':len(history),'pid':os.getpid(),'resumed':resumed})
  event('item/completed',{'threadId':session,'item':{'id':turn+'-answer','type':'agentMessage','phase':'final_answer','text':result}})
  event('turn/completed',{'threadId':session,'turn':{'id':turn,'status':'completed'}})
  if len(history)==1:
   event('item/agentMessage/delta',{'threadId':'child-native','itemId':'child-answer','delta':'late child answer'})
   event('turn/completed',{'threadId':'child-native','turn':{'id':'child-turn','status':'completed'}})
 else:reply(v,{})
