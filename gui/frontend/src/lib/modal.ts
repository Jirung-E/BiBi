import {refreshScrollbars} from './scrollbars';

// The app viewport is fixed. Prevent wheel/touch scroll chaining into its
// internal panels while the topmost dialog is open.
const dialogs:HTMLDialogElement[]=[];
function blockBackground(event:Event){const active=dialogs.at(-1);if(active&&event.target instanceof Node&&!active.contains(event.target))event.preventDefault();}
// Reference counting covers overlapping dialogs and idempotent cleanup.
const locks = new WeakMap<HTMLElement, { count: number; restore: () => void }>();

export function lockPageScroll(root: HTMLElement): () => void {
  let lock = locks.get(root);
  if (!lock) {
    const properties = ['overflow', 'overscroll-behavior'];
    const previous = properties.map(name => [name, root.style.getPropertyValue(name), root.style.getPropertyPriority(name)]);
    root.style.setProperty('overflow', 'hidden');
    root.style.setProperty('overscroll-behavior', 'none');
    lock = { count: 0, restore() {
      for (const [name, value, priority] of previous) {
        if (value) root.style.setProperty(name, value, priority);
        else root.style.removeProperty(name);
      }
    } };
    locks.set(root, lock);
  }
  lock.count++;
  let released = false;
  return () => {
    if (released) return;
    released = true;
    if (--lock.count === 0) { lock.restore(); locks.delete(root); }
  };
}

export function modalDialog(node: HTMLDialogElement,options:{returnFocus?:()=>HTMLElement|undefined}={}) {
  const unlock = lockPageScroll(node.ownerDocument.documentElement);
  let released=false;
  const release=()=>{
    if(released)return;released=true;
    const index=dialogs.indexOf(node);if(index>=0)dialogs.splice(index,1);
    if(!dialogs.length){document.removeEventListener('wheel',blockBackground,true);document.removeEventListener('touchmove',blockBackground,true);}
    unlock();refreshScrollbars();
    // An outgoing dialog stays in the top layer until its animation ends.
    // Restore focus after teardown, unless another dialog has taken over.
    if(options.returnFocus)queueMicrotask(()=>{
      const target=options.returnFocus?.();
      if(!dialogs.length&&target?.isConnected)target.focus({preventScroll:true});
    });
  };
  node.addEventListener('close', release);
  try {
    node.showModal();dialogs.push(node);
    document.addEventListener('wheel',blockBackground,{capture:true,passive:false});
    document.addEventListener('touchmove',blockBackground,{capture:true,passive:false});
    refreshScrollbars();
  } catch (error) { node.removeEventListener('close', release); release(); throw error; }
  return { destroy() {
    node.removeEventListener('close', release);
    node.close();
    release();
  } };
}
