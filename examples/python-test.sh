#!/usr/bin/env python3

import sys
import io

for i, arg in enumerate(sys.argv):
    print("Argument #{}: {}".format(i, arg))
    with open(arg, 'rb') as f:
        f.seek(0, io.SEEK_END)
        print("File size is {} bytes.".format(f.tell()))
sys.exit(1)
