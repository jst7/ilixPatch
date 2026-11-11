/** CSS colour variable for an HTTP method badge. */
export function methodColor(method: string): string {
  switch (method.toUpperCase()) {
    case "GET":
      return "var(--method-get)";
    case "POST":
      return "var(--method-post)";
    case "PUT":
      return "var(--method-put)";
    case "PATCH":
      return "var(--method-patch)";
    case "DELETE":
      return "var(--method-delete)";
    default:
      return "var(--method-other)";
  }
}

/** Short label so badges keep a fixed width. */
export function methodLabel(method: string): string {
  const m = method.toUpperCase();
  return m === "DELETE" ? "DEL" : m === "OPTIONS" ? "OPT" : m;
}
