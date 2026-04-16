; Process with static string

(begin
  (setq s "Hello")
  (print-str s)
  (print-str "\n")
  (print (strlen s))
  (print-str "\n")
  (strset s 0 89)
  (print-str s)
  (halt))
