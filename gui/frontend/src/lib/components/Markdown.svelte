<script lang="ts">
import {markdown} from '../markdown';
import {tableCopies} from '../table-copy';
import {mermaidBlocks} from '../mermaid-blocks';
import {contentScrollbars} from '../scrollbars';
let {text}:{text:string}=$props();
const html=$derived(markdown(text));
</script>
<div class="message-text markdown" use:contentScrollbars use:mermaidBlocks={html} use:tableCopies={html}>{@html html}</div>
