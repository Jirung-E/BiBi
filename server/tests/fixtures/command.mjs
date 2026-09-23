let input='';for await(const chunk of process.stdin)input+=chunk;
const request=JSON.parse(input);
if(request.prompt==='HOLD_CLOSED_STDOUT'){
 process.stdout.end();setInterval(()=>{},1000);
}else{
 const answer=JSON.stringify({answer:'한글 답변',prompt:request.prompt,turns:request.messages.filter(m=>m.role==='user').length,previous:request.messages.some(m=>m.role==='assistant'),model:request.model});
 for(const byte of Buffer.from(answer))process.stdout.write(Buffer.from([byte]));
}
