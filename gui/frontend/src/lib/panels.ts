export const panelDefaults = {sidebar:18,inspector:22,context:22,inspectorHeight:20,contextHeight:16};
export type PanelSizes = typeof panelDefaults;
export type PanelName = keyof PanelSizes;
export const panelLimits:Record<PanelName,readonly [number,number]> = {
 sidebar:[14,26],inspector:[18,32],context:[18,32],inspectorHeight:[6,30],contextHeight:[6,30]
};
export function readPanelSizes(raw:string|null):PanelSizes {
 const sizes={...panelDefaults};
 try {
  const saved=JSON.parse(raw??'null');
  for(const name of Object.keys(sizes) as PanelName[]){
   const value=saved?.[name],[min,max]=panelLimits[name];
   if(typeof value==='number'&&Number.isFinite(value))sizes[name]=Math.max(min,Math.min(max,value));
  }
 }catch{/* A stale or unavailable preference must not block navigation. */}
 return sizes;
}
