export type Point = { x:number; y:number };
export type Viewport = { pan:Point; zoom:number };
export const NODE_WIDTH=252, NODE_HEIGHT=106;
export function zoomAt(view:Viewport, point:Point, level:number):Viewport {
  const zoom=Math.max(Math.min(0.02,view.zoom),Math.min(2,level));
  return {zoom,pan:{x:point.x-(point.x-view.pan.x)/view.zoom*zoom,y:point.y-(point.y-view.pan.y)/view.zoom*zoom}};
}
export function opacity(sentAt:number,now:number,halfLife=30,floor=0.15):number {
  return floor+(1-floor)*Math.pow(2,-Math.max(0,now-sentAt)/1000/Math.max(1,halfLife));
}
export function edgePath(from:Point,to:Point,reverse=false):string {
  const dx=to.x-from.x;
  const start={x:from.x+(dx>=0?NODE_WIDTH:0),y:from.y+NODE_HEIGHT/2+(reverse?12:-8)};
  const end={x:to.x+(dx>=0?0:NODE_WIDTH),y:to.y+NODE_HEIGHT/2+(reverse?12:-8)};
  const bend=Math.max(60,Math.abs(end.x-start.x)*0.42)*(dx>=0?1:-1);
  return 'M '+start.x+' '+start.y+' C '+(start.x+bend)+' '+start.y+', '+(end.x-bend)+' '+end.y+', '+end.x+' '+end.y;
}
export function fit(points:Point[],width:number,height:number):Viewport {
  if (!points.length) return {pan:{x:24,y:40},zoom:1};
  const x=Math.min(...points.map(p=>p.x))-24,y=Math.min(...points.map(p=>p.y))-32;
  const w=Math.max(...points.map(p=>p.x))+NODE_WIDTH+24-x,h=Math.max(...points.map(p=>p.y))+NODE_HEIGHT+24-y;
  const zoom=Math.min(1,Math.max(1,width-32)/w,Math.max(1,height-32)/h);
  return {zoom,pan:{x:(width-w*zoom)/2-x*zoom,y:(height-h*zoom)/2-y*zoom}};
}
