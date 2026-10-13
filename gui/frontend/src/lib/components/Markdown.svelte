<script lang="ts">
import {messageMarkdown} from '../markdown';
import {htmlPieces,stableHtml} from '../stable-html';
import type {Attachment} from '../types';
import {attachmentLinks} from '../attachment-links';
import {tableCopies} from '../table-copy';
import {mermaidBlocks} from '../mermaid-blocks';
import {contentScrollbars} from '../scrollbars';
let {text,attachments=[]}:{text:string;attachments?:Attachment[]}=$props();
const render=messageMarkdown();
const html=$derived(render(text,attachments));
const pieces=$derived(htmlPieces(html));
</script>
<div class="message-text markdown" use:attachmentLinks use:contentScrollbars use:mermaidBlocks={html} use:tableCopies={html}>{#each pieces as piece,i(i)}<div class="markdown-piece" use:stableHtml={piece}></div>{/each}</div>
