import { useHttpScheme } from "./platform";

export function schemeUrl(scheme: string, path = ""): string {
  return useHttpScheme
    ? `http://${scheme}.localhost/${path}`
    : `${scheme}://localhost/${path}`;
}
