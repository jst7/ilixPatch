// While a pointer drag is in progress the pointer crosses other text, and the webview would
// select it. Call at drag start; the returned function restores selection at the end.
export function suppressTextSelection(): () => void {
  const body = document.body.style;
  const prev = [body.userSelect, body.webkitUserSelect];
  body.userSelect = "none";
  body.webkitUserSelect = "none";
  window.getSelection()?.removeAllRanges();
  return () => {
    body.userSelect = prev[0];
    body.webkitUserSelect = prev[1];
    window.getSelection()?.removeAllRanges();
  };
}
