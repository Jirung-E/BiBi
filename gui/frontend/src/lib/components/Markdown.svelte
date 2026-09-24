<script lang="ts">
import {markdown} from '../markdown';
import {contentScrollbars} from '../scrollbars';
let {text}:{text:string}=$props();
const html=$derived(markdown(text));
</script>
<div class="message-text markdown" use:contentScrollbars>{@html html}</div>
