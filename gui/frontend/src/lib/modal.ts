// The document is BiBi's page scroll owner. Reference counting also covers
// overlapping dialogs during navigation and makes cleanup safe to repeat.
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

export function modalDialog(node: HTMLDialogElement) {
  const release = lockPageScroll(node.ownerDocument.documentElement);
  node.addEventListener('close', release);
  try { node.showModal(); } catch (error) { node.removeEventListener('close', release); release(); throw error; }
  return { destroy() {
    node.removeEventListener('close', release);
    node.close();
    release();
  } };
}
