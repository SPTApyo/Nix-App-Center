"""Interactive helper: drives an already running Broadway session.

Usage: python3 session.py OUT.png CMD...
CMD: click:X,Y | type:TEXT | key:NAME | wait:MS | scroll:DY@X,Y
"""
import sys

from nix_app_center_e2e import Session

with Session.attach() as s:
    for cmd in sys.argv[2:]:
        s.do(cmd)
    s.shot(sys.argv[1])
