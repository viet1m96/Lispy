;selection sort characters in a static string

(begin
  (setq s :string "dbca")
  (setq n :int (strlen s))
  (setq i :int 0)

  (loop while (< i n) do
    (setq best :int i)
    (setq j :int (+ i 1))

    (loop while (< j n) do
      (if (< (strget s j) (strget s best))
          (setq best :int j)
          0)

      (setq j :int (+ j 1))
    finally 0)

    (if (!= best i)
        (let ((tmp :int (strget s i)))
          (begin
            (strset s i (strget s best))
            (strset s best tmp)))
        0)

    (setq i :int (+ i 1))
  finally 0)

  (print-str s)
  (halt))
