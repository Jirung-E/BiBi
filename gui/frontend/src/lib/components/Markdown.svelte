<script lang="ts">
import {messageMarkdown} from '../markdown';
import type {Attachment} from '../types';
import {attachmentLinks} from '../attachment-links';
import {tableCopies} from '../table-copy';
import {mermaidBlocks} from '../mermaid-blocks';
import {contentScrollbars} from '../scrollbars';
let {text,attachments=[]}:{text:string;attachments?:Attachment[]}=$props();
const render=messageMarkdown();
const html=$derived(render(text,attachments));
</script>
<div class="message-text markdown" use:attachmentLinks use:contentScrollbars use:mermaidBlocks={html} use:tableCopies={html}>{@html html}</div>
