// Which stored OAuth 2.0 token the user picked per token name ("literal" = typed access_token).
// The backend also remembers it (oauth2SelectToken); this mirrors it for the UI during the session.
class OAuth2Selection {
  private byName = $state<Record<string, string>>({});

  get(tokenName: string): string | null {
    return this.byName[tokenName] ?? null;
  }

  set(tokenName: string, value: string) {
    this.byName[tokenName] = value;
  }
}

export const oauth2Selection = new OAuth2Selection();
