library(tidyverse)
library(hrbrthemes)

dta <- read_csv("out.log", col_types = "nniiic") %>%
    filter(tx_time != 0) %>%
    mutate(sim_time = sim_time/1000,
           rx_time = (rx_time)/1000000,
           tx_time = (tx_time)/1000000,
           src = as.factor(src),
           id  = as.factor(id),
           start = ifelse(type != "ModelEvent(Packet)", sim_time-.1, sim_time-.1-1.5))

dta %>%
    group_by(type, id) %>%
    summarize(count = n()) %>%
    ggplot(aes(x = type,
               y = count,
               fill = type)) +
    geom_col() +
    facet_wrap(~id)

start_t <- round(runif(n = 1, min = 0, max = max(dta$sim_time)))
#start_t <- 20
duration <- 250
end <- start_t + duration
#end <- 450

#start_sim <- round(runif(n = 1, min = 0, max = max(dta$sim_time)))
start_sim <- 000
end_sim <- start_sim + 50
#end_sim <- max(dta$sim_time)

dta %>%
    filter(src != 0) %>%
    filter(sim_time >= start_sim, sim_time <= end_sim) %>%
    #filter(tx_time > start_t | rx_time > start_t,
           #tx_time < end | rx_time < end) %>%
    sample_n(min(100000, nrow(.))) %>%
    #filter(type == "Null") %>% #| type == "Stalled") %>%
    ggplot(aes(x = tx_time,
               y = start,
               xend = rx_time,
               yend = sim_time,
               color = type)) +
    # geom_step(aes(x = tx_time,
    #               y = start,
    #               group = paste(src)),
    #           color = "orange") +
    geom_step(aes(x = rx_time,
                  y = sim_time,
                  group = id),
              color = "black") +
    geom_segment(arrow = arrow(length = unit(.05, "inches")),
                 size = .5) +
    #coord_cartesian(xlim = c(start_t, end)) +
    labs(x = "Real time (ms)",
         y = "Simulation time (us)") +
    #geom_point() +
    facet_wrap(~id) +
    hrbrthemes::theme_ipsum_rc()

dta %>%
    sample_n(100000) %>%
    ggplot(aes(x = tx_time,
               y = rx_time,
               color = id,
               group = paste(src, id))) +
    geom_step()

dta %>%
    sample_n(100000) %>%
    #filter(time < end) %>%
    ggplot(aes(x = real_time,
               y = sim_time,
               group = src,
               color = type)) +
    geom_step() +
    labs(y = "Sim time (us)",
         x = "Real time",
         color = "Event dest")
    #geom_point(size = .3)

max_fn <- function(size) {
    6*500+size*8/10
}

dta <- read_csv("flows1perc.csv")
max(dta$end)/1e9
nrow(dta)

control <- read_delim("~/opera-sim/Figure7_datamining/3to1_clos/sim/FCT_3to1_pfab_cwnd30_25perc.txt",
                      delim = " ", skip = 1,
                      col_names = c("type", "src", "dst", "size_byte", "fct_ms", "start_ms"),
                      comment = "Util")

control_fcts <- control %>%
    mutate(fct_ns = 1e6*fct_ms) %>%
    group_by(size_byte) %>%
        summarize(med = median(fct_ns),
                  perc90 = quantile(fct_ns, .9),
                  perc99 = quantile(fct_ns, .99)) %>%
        ungroup() %>%
    mutate(source = "control")

experiment_fcts <- dta %>%
    mutate(fct_ns = fct_ns - 500*4) %>%
    group_by(size_byte) %>%
        summarize(med = median(fct_ns),
                  perc90 = quantile(fct_ns, .9),
                  perc99 = quantile(fct_ns, .99)) %>%
        ungroup() %>%
    mutate(source = "experiment")

fcts <- rbind(control_fcts, experiment_fcts) %>%
    reshape2::melt(id.vars = c("size_byte", "source"),
                   variable.name = "statistic",
                   value.name = "fct_ns") %>%
    as_tibble()

fcts %>%
    ggplot(aes(x = size_byte,
               y = fct_ns,
               color = source,
               group = paste(source, statistic))) +
    # geom_line(data = tibble(size = seq(from = log(min(dta$size_byte)), to = log(max(dta$size_byte)), length.out = 100) %>% exp()) %>%
    #               mutate(min_fct = max_fn(size)),
    #           inherit.aes = FALSE,
    #           aes(x = size, y = min_fct),
    #           color = "black",
    #           linetype = "dashed") +
    geom_point(shape = "cross", size = 5) +
    geom_line() +

    scale_y_log10(breaks = 10^(2:10),
                  labels = c("100ns", "1us", "10us", "100us", "1ms", "10ms", "100ms", "1s", "10s")) +
    scale_x_log10(breaks = 10^(2:10),
                  labels = c("100 B", "1 KB", "10 KB", "100 KB", "1 MB", "10 MB", "100 MB", "1 GB", "10 GB")) +

    facet_wrap(~statistic) +

    labs(x = NULL, y = NULL,
         caption = "github.com/nibrivia/rustasim",
         title = "Flow completion time by flow size",
         subtitle = paste0("3:1 CLOS topology, k=12 switches, 25% load (n=", prettyNum(nrow(dta), big.mark = ","), ")")) +
    theme_ipsum_rc()

one_to_one <- control %>%
    mutate(fct_ns = 1e6*fct_ms,
           start = 1e6*start_ms,
           src = src+1,
           dst = dst+1,
           start_us = round(1e3*start_ms)) %>%
    select(-fct_ms, -start_ms, -type) %>%
    full_join(dta %>% mutate(start_us = round(start/1e3)),
              by = c("src", "dst", "size_byte", "start_us"),
              suffix = c(".control", ".experiment"))

one_to_one %>%
    mutate(fct_ns.experiment = end - start.control,
           diff = (fct_ns.experiment) / (1436/1500*fct_ns.control)) %>%
    filter(!is.na(diff), size_byte > 2*1500) %>%
    group_by(size_byte) %>%
        summarize(diff = diff %>% log %>% mean %>% exp,
                  n = n()) %>%
        ungroup() %>%
    ggplot(aes(x = size_byte,
               y = diff)) +
    geom_point() +
    geom_line() +
    scale_x_log10() +
    scale_y_log10()
