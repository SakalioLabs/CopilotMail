#!/usr/bin/env python3
import argparse, json, os, sys, urllib.error, urllib.request

BASE = os.environ.get("COPILOTMAIL_URL", "http://127.0.0.1:8080").rstrip("/")

def request(method, path, body=None, token=None, admin_key=None):
    headers = {"Accept": "application/json"}
    data = None
    if body is not None:
        headers["Content-Type"] = "application/json"
        data = json.dumps(body).encode()
    if token: headers["Authorization"] = "Bearer " + token
    if admin_key: headers["X-Admin-Key"] = admin_key
    try:
        with urllib.request.urlopen(urllib.request.Request(BASE + path, data=data, headers=headers, method=method)) as response:
            payload = response.read()
            return json.loads(payload) if payload else {"status": response.status}
    except urllib.error.HTTPError as error:
        print(error.read().decode(errors="replace"), file=sys.stderr)
        raise SystemExit(error.code)

def main():
    parser = argparse.ArgumentParser(description="CopilotMail API client")
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("health")
    create = sub.add_parser("create"); create.add_argument("address"); create.add_argument("--password", required=True); create.add_argument("--admin-key", default=os.environ.get("COPILOTMAIL_ADMIN_KEY"), required=os.environ.get("COPILOTMAIL_ADMIN_KEY") is None)
    login = sub.add_parser("login"); login.add_argument("address"); login.add_argument("--password", required=True)
    inbox = sub.add_parser("inbox"); inbox.add_argument("--token", default=os.environ.get("COPILOTMAIL_TOKEN"), required=os.environ.get("COPILOTMAIL_TOKEN") is None)
    send = sub.add_parser("send"); send.add_argument("--token", default=os.environ.get("COPILOTMAIL_TOKEN"), required=os.environ.get("COPILOTMAIL_TOKEN") is None); send.add_argument("--to", required=True); send.add_argument("--subject", required=True); send.add_argument("--text", required=True)
    args = parser.parse_args()
    if args.command == "health": result = request("GET", "/healthz")
    elif args.command == "create": result = request("POST", "/v1/mailboxes", {"address":args.address,"password":args.password}, admin_key=args.admin_key)
    elif args.command == "login": result = request("POST", "/v1/auth/token", {"address":args.address,"password":args.password})
    elif args.command == "inbox": result = request("GET", "/v1/messages", token=args.token)
    else: result = request("POST", "/v1/messages/send", {"to":args.to,"subject":args.subject,"text":args.text}, token=args.token)
    print(json.dumps(result, ensure_ascii=False, indent=2))

if __name__ == "__main__": main()
