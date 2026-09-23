// @vitest-environment jsdom
import { expect, it } from 'vitest';
import { lockPageScroll, modalDialog } from './modal';

it('keeps the page locked until all dialogs close and restores the original styles', () => {
  const root = document.createElement('div');
  root.style.setProperty('overflow', 'auto', 'important');
  root.style.setProperty('overscroll-behavior', 'contain');
  const first = lockPageScroll(root), second = lockPageScroll(root);
  first(); first();
  expect(root.style.overflow).toBe('hidden');
  second();
  expect(root.style.overflow).toBe('auto');
  expect(root.style.getPropertyPriority('overflow')).toBe('important');
  expect(root.style.getPropertyValue('overscroll-behavior')).toBe('contain');
});

it('releases a lock if opening the native dialog fails', () => {
  const dialog = document.createElement('dialog');
  dialog.showModal = () => { throw new Error('disconnected dialog'); };
  expect(() => modalDialog(dialog)).toThrow('disconnected dialog');
  expect(document.documentElement.style.overflow).toBe('');
});
