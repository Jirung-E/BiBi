export type Point = { x:number; y:number };
export type Size={width:number;height:number};
export const nodeSize=(kind:string,scale=1):Size=>({width:(kind==='subagent'?196:252)*scale,height:(kind==='subagent'?84:118)*scale});
export type Viewport = { pan:Point; zoom:number };
export const NODE_WIDTH=252, NODE_HEIGHT=118;
export function zoomAt(view:Viewport, point:Point, level:number):Viewport {
  const zoom=Math.max(Math.min(0.02,view.zoom),Math.min(2,level));
  return {zoom,pan:{x:point.x-(point.x-view.pan.x)/view.zoom*zoom,y:point.y-(point.y-view.pan.y)/view.zoom*zoom}};
}
// Wheel units differ across browsers. Bound a physical mouse notch while
// preserving the small fractional deltas of a trackpad or pinch gesture.
export function wheelZoomFactor(delta:number,mode=0):number {
  if(!Number.isFinite(delta))return 1;
  const pixels=delta*(mode===1?40:mode===2?120:1);
  return Math.exp(-Math.max(-120,Math.min(120,pixels))*.001);
}
export function opacity(sentAt:number,now:number,halfLife=30,floor=0.15):number {
  return floor+(1-floor)*Math.pow(2,-Math.max(0,now-sentAt)/1000/Math.max(1,halfLife));
}
export function edgePath(from:Point,to:Point,reverse=false,fromSize:Size={width:NODE_WIDTH,height:NODE_HEIGHT},toSize:Size={width:NODE_WIDTH,height:NODE_HEIGHT}):string {
  const dx=to.x-from.x;
  const start={x:from.x+(dx>=0?fromSize.width:0),y:from.y+fromSize.height/2+(reverse?12:-8)};
  const end={x:to.x+(dx>=0?0:toSize.width),y:to.y+toSize.height/2+(reverse?12:-8)};
  const bend=Math.max(60,Math.abs(end.x-start.x)*0.42)*(dx>=0?1:-1);
  return 'M '+start.x+' '+start.y+' C '+(start.x+bend)+' '+start.y+', '+(end.x-bend)+' '+end.y+', '+end.x+' '+end.y;
}
export function fit(points:(Point&Partial<Size>)[],width:number,height:number):Viewport {
  if (!points.length) return {pan:{x:24,y:40},zoom:1};
  const x=Math.min(...points.map(p=>p.x))-24,y=Math.min(...points.map(p=>p.y))-32;
  const w=Math.max(...points.map(p=>p.x+(p.width??NODE_WIDTH)))+24-x,h=Math.max(...points.map(p=>p.y+(p.height??NODE_HEIGHT)))+24-y;
  const zoom=Math.min(1,Math.max(1,width-32)/w,Math.max(1,height-32)/h);
  return {zoom,pan:{x:(width-w*zoom)/2-x*zoom,y:(height-h*zoom)/2-y*zoom}};
}

export function translateGroup(points:Record<string,Point>,members:string[],delta:Point):Record<string,Point>{
 const next={...points};for(const id of members){if(next[id])next[id]={x:next[id].x+delta.x,y:next[id].y+delta.y};}return next;
}

// Persist positions at 100% UI scale. Sizes and positions enter the same
// display coordinate system before canvas zoom/pan is applied.
export function scalePoints(points:Record<string,Point>,scale:number):Record<string,Point>{
 return Object.fromEntries(Object.entries(points).map(([id,p])=>[id,{x:p.x*scale,y:p.y*scale}]));
}
export function dragDelta(delta:Point,zoom:number,uiScale:number):Point{
 return {x:delta.x/(zoom*uiScale),y:delta.y/(zoom*uiScale)};
}

export type CameraFrame={view:Viewport;size:Size};
// Always project from the same frame during layout changes. Chaining min-ratios
// from each ResizeObserver callback would shrink again on every open/close.
export function resizeViewport(frame:CameraFrame,size:Size):Viewport{
 if(frame.size.width<=0||frame.size.height<=0||size.width<=0||size.height<=0)return frame.view;
 const zoom=frame.view.zoom*Math.min(size.width/frame.size.width,size.height/frame.size.height);
 const center={x:(frame.size.width/2-frame.view.pan.x)/frame.view.zoom,y:(frame.size.height/2-frame.view.pan.y)/frame.view.zoom};
 return {zoom,pan:{x:size.width/2-center.x*zoom,y:size.height/2-center.y*zoom}};
}
