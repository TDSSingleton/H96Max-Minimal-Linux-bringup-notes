package main

// M3U / M3U8 (extended) playlist parsing: #EXTINF:-1 tvg-id=".." group-title="..",Name
// followed by the stream URL. Plain URL lines without #EXTINF are accepted too.

import (
	"bufio"
	"io"
	"regexp"
	"strings"
)

type Channel struct {
	Name  string
	Group string
	URL   string
}

var attrRe = regexp.MustCompile(`([A-Za-z0-9_-]+)="([^"]*)"`)

func ParseM3U(r io.Reader) []Channel {
	var out []Channel
	var pending *Channel
	sc := bufio.NewScanner(r)
	sc.Buffer(make([]byte, 64*1024), 4*1024*1024)
	first := true
	for sc.Scan() {
		line := strings.TrimSpace(sc.Text())
		if first {
			line = strings.TrimPrefix(line, "\ufeff")
			first = false
		}
		switch {
		case line == "":
			continue
		case strings.HasPrefix(line, "#EXTINF"):
			c := &Channel{}
			head := line
			if i := lastTopLevelComma(line); i >= 0 {
				c.Name = strings.TrimSpace(line[i+1:])
				head = line[:i]
			}
			for _, m := range attrRe.FindAllStringSubmatch(head, -1) {
				switch strings.ToLower(m[1]) {
				case "group-title":
					c.Group = m[2]
				case "tvg-name":
					if c.Name == "" {
						c.Name = m[2]
					}
				}
			}
			pending = c
		case strings.HasPrefix(line, "#EXTGRP:"):
			if pending != nil && pending.Group == "" {
				pending.Group = strings.TrimSpace(strings.TrimPrefix(line, "#EXTGRP:"))
			}
		case strings.HasPrefix(line, "#"):
			continue
		default:
			c := Channel{URL: line}
			if pending != nil {
				c.Name, c.Group = pending.Name, pending.Group
			}
			if c.Name == "" {
				c.Name = line
			}
			out = append(out, c)
			pending = nil
		}
	}
	return out
}

// lastTopLevelComma finds the comma that separates attributes from the title, ignoring
// commas inside quoted attribute values.
func lastTopLevelComma(s string) int {
	inQ := false
	idx := -1
	for i, r := range s {
		switch r {
		case '"':
			inQ = !inQ
		case ',':
			if !inQ {
				idx = i
				return idx // first comma outside quotes ends the attribute part
			}
		}
	}
	return idx
}
