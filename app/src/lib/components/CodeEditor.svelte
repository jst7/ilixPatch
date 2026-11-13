<script lang="ts">
  import { onMount } from "svelte";
  import { Annotation, Compartment, EditorState, type Extension } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";
  import { foldAll as cmFoldAll, unfoldAll as cmUnfoldAll } from "@codemirror/language";
  import { i18n } from "$lib/i18n/index.svelte";
  import {
    staticExtensions,
    themeExtension,
    phrasesExtension,
    languageExtension,
    modeExtension,
    placeholderExtension,
    type EditorLanguage,
  } from "$lib/editor/setup";
  import { openFind } from "$lib/editor/searchPanel";
  import { blameExtension, setBlame } from "$lib/editor/blame";
  import type { BlameInfo } from "$lib/api";

  interface Props {
    value: string;
    onchange?: (value: string) => void;
    language?: EditorLanguage;
    readonly?: boolean;
    placeholder?: string;
    /** Git annotate: one entry per document line (null/undefined hides the gutter). */
    annotations?: BlameInfo[] | null;
    /** Called when a committed line's annotation is clicked. */
    onannotationclick?: (info: BlameInfo) => void;
    /** Extra extensions (decorations, fields…), applied once at creation; not reconfigured later. */
    extensions?: Extension[];
    /** Called with the view once it is created, and with null when it is destroyed. */
    onview?: (view: EditorView | null) => void;
    /** Wrap long lines instead of scrolling horizontally. */
    wrap?: boolean;
  }

  let {
    value,
    onchange,
    language = "json",
    readonly = false,
    placeholder,
    annotations = null,
    onannotationclick,
    extensions = [],
    onview,
    wrap = false,
  }: Props = $props();

  let host: HTMLDivElement;
  let view: EditorView | undefined;
  // Last document text known to be in sync with `value` (avoids doc.toString() on every effect run).
  let syncedValue = "";

  // Marks transactions that originate from the `value` prop (must not fire onchange).
  const External = Annotation.define<boolean>();

  // Values currently applied to the view; prevents redundant reconfigures
  // (re-creating the JSON language would force a full re-parse of large docs).
  let appliedLanguage: EditorLanguage = "json";
  let appliedReadonly = false;
  let appliedPlaceholder: string | undefined;

  const phrasesC = new Compartment();
  const langC = new Compartment();
  const modeC = new Compartment();
  const placeholderC = new Compartment();
  const blameC = new Compartment();
  const wrapC = new Compartment();
  let appliedWrap = false;
  // Annotations currently applied, and whether the blame gutter is installed.
  let appliedAnnotations: BlameInfo[] | null = null;
  let blameInstalled = false;
  const blameExt = (a: BlameInfo[]) => blameExtension(a, { onclick: (info) => onannotationclick?.(info) });

  onMount(() => {
    syncedValue = value;
    appliedLanguage = language;
    appliedReadonly = readonly;
    appliedPlaceholder = placeholder;
    appliedLocale = i18n.locale;
    appliedAnnotations = annotations;
    blameInstalled = !!annotations;
    appliedWrap = wrap;

    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: value,
        extensions: [
          staticExtensions(),
          themeExtension(),
          phrasesC.of(phrasesExtension()),
          langC.of(languageExtension(language)),
          modeC.of(modeExtension(readonly)),
          placeholderC.of(placeholderExtension(placeholder)),
          blameC.of(annotations ? blameExt(annotations) : []),
          wrapC.of(wrap ? EditorView.lineWrapping : []),
          EditorView.updateListener.of((u) => {
            if (!u.docChanged) return;
            if (u.transactions.every((tr) => tr.annotation(External))) return;
            syncedValue = u.state.doc.toString();
            onchange?.(syncedValue);
          }),
          extensions,
        ],
      }),
    });
    onview?.(view);

    return () => {
      onview?.(null);
      view?.destroy();
      view = undefined;
    };
  });

  // External value updates → replace whole doc in one transaction, no onchange.
  $effect(() => {
    const v = value;
    if (!view || v === syncedValue) return;
    syncedValue = v;
    if (v === view.state.doc.toString()) return;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: v },
      annotations: [External.of(true)],
    });
  });

  // Re-localise search panel / fold widgets when the app locale changes.
  let appliedLocale = i18n.locale;
  $effect(() => {
    const loc = i18n.locale;
    if (!view || loc === appliedLocale) return;
    appliedLocale = loc;
    view.dispatch({ effects: phrasesC.reconfigure(phrasesExtension()) });
  });

  $effect(() => {
    const l = language;
    if (!view || l === appliedLanguage) return;
    appliedLanguage = l;
    view.dispatch({ effects: langC.reconfigure(languageExtension(l)) });
  });

  $effect(() => {
    const r = readonly;
    if (!view || r === appliedReadonly) return;
    appliedReadonly = r;
    view.dispatch({ effects: modeC.reconfigure(modeExtension(r)) });
  });

  $effect(() => {
    const w = wrap;
    if (!view || w === appliedWrap) return;
    appliedWrap = w;
    view.dispatch({ effects: wrapC.reconfigure(w ? EditorView.lineWrapping : []) });
  });

  $effect(() => {
    const p = placeholder;
    if (!view || p === appliedPlaceholder) return;
    appliedPlaceholder = p;
    view.dispatch({ effects: placeholderC.reconfigure(placeholderExtension(p)) });
  });

  // Git annotate gutter: install/remove via compartment, update data via effect.
  $effect(() => {
    const a = annotations;
    if (!view || a === appliedAnnotations) return;
    appliedAnnotations = a;
    if (!a) {
      if (blameInstalled) view.dispatch({ effects: blameC.reconfigure([]) });
      blameInstalled = false;
    } else if (!blameInstalled) {
      view.dispatch({ effects: blameC.reconfigure(blameExt(a)) });
      blameInstalled = true;
    } else {
      view.dispatch({ effects: setBlame.of(a) });
    }
  });

  export function foldAll() {
    if (view) cmFoldAll(view);
  }

  export function unfoldAll() {
    if (view) cmUnfoldAll(view);
  }

  export function openSearch() {
    if (view) openFind(view);
  }

  export function focus() {
    view?.focus();
  }

  /** Insert text at the cursor (replacing the selection) on its own lines, then focus. Fires onchange. */
  export function insert(text: string) {
    if (!view) return;
    const { from, to } = view.state.selection.main;
    const doc = view.state.doc;
    const before = from > 0 && doc.sliceString(from - 1, from) !== "\n" ? "\n" : "";
    const after = to < doc.length && doc.sliceString(to, to + 1) !== "\n" ? "\n" : "";
    const insert = before + text + after;
    view.dispatch({
      changes: { from, to, insert },
      selection: { anchor: from + before.length + text.length },
      scrollIntoView: true,
    });
    view.focus();
  }
</script>

<div class="code-editor" bind:this={host}></div>

<style>
  .code-editor {
    height: 100%;
    min-height: 0;
    width: 100%;
    min-width: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .code-editor :global(.cm-editor) {
    flex: 1 1 auto;
    min-height: 0;
  }
</style>
