#!/bin/sh

openssl genpkey -algorithm RSA -out key.pem
openssl pkcs8 -in key.pem -topk8 -out pk8key.pem